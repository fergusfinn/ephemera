use askama::Template;
use axum::{
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePool, migrate::MigrateDatabase};

#[derive(Deserialize)]
struct PostMetricQuery {
    value: f64,
}

#[derive(Deserialize)]
struct PaginationQuery {
    page: Option<u32>,
}

#[derive(Serialize)]
struct MetricPoint {
    timestamp: i64,
    value: f64,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate;

#[derive(Template)]
#[template(path = "chart.html")]
struct ChartTemplate {
    namespace: String,
    id: String,
    data_json: String,
}

#[derive(Template)]
#[template(path = "namespace.html")]
struct NamespaceTemplate {
    namespace: String,
    charts: Vec<ChartInfo>,
    current_page: u32,
    total_pages: u32,
    has_prev: bool,
    has_next: bool,
}

#[derive(Serialize)]
struct ChartInfo {
    id: String,
    point_count: i64,
    last_updated: String,
}

async fn post_metric(
    Path((namespace, id)): Path<(String, String)>,
    Query(params): Query<PostMetricQuery>,
    pool: axum::extract::State<SqlitePool>,
) -> Result<impl IntoResponse, StatusCode> {
    let timestamp = Utc::now().timestamp();
    
    let result = sqlx::query!(
        "INSERT INTO metrics (namespace, id, value, timestamp) VALUES (?, ?, ?, ?)",
        namespace,
        id,
        params.value,
        timestamp
    )
    .execute(&*pool)
    .await;
    
    match result {
        Ok(_) => Ok(StatusCode::OK),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_chart(
    Path((namespace, id)): Path<(String, String)>,
    pool: axum::extract::State<SqlitePool>,
) -> Result<impl IntoResponse, StatusCode> {
    let rows = sqlx::query!(
        "SELECT value, timestamp FROM metrics WHERE namespace = ? AND id = ? ORDER BY timestamp ASC",
        namespace,
        id
    )
    .fetch_all(&*pool)
    .await;
    
    let data = match rows {
        Ok(rows) => rows
            .into_iter()
            .map(|row| MetricPoint {
                timestamp: row.timestamp,
                value: row.value,
            })
            .collect::<Vec<_>>(),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    
    let data_json = serde_json::to_string(&data).unwrap_or_default();
    
    let template = ChartTemplate {
        namespace,
        id,
        data_json,
    };
    
    match template.render() {
        Ok(html) => Ok(Html(html)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_index() -> Result<impl IntoResponse, StatusCode> {
    let template = IndexTemplate;
    match template.render() {
        Ok(html) => Ok(Html(html)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_favicon() -> impl IntoResponse {
    const FAVICON_SVG: &str = include_str!("../favicon.svg");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "image/svg+xml")
        .body(FAVICON_SVG.to_string())
        .unwrap()
}

async fn get_namespace(
    Path(namespace): Path<String>,
    Query(pagination): Query<PaginationQuery>,
    pool: axum::extract::State<SqlitePool>,
) -> Result<impl IntoResponse, StatusCode> {
    let page = pagination.page.unwrap_or(1).max(1);
    let per_page = 12; // Show 12 charts per page (nice grid layout)
    let offset = (page - 1) * per_page;
    
    // Get total count for pagination
    let total_count = sqlx::query!(
        "SELECT COUNT(DISTINCT id) as count FROM metrics WHERE namespace = ?",
        namespace
    )
    .fetch_one(&*pool)
    .await
    .map(|row| row.count as u32)
    .unwrap_or(0);
    
    let total_pages = (total_count + per_page - 1) / per_page; // Ceiling division
    
    let rows = sqlx::query!(
        r#"
        SELECT 
            id,
            COUNT(*) as "point_count: i64",
            MAX(timestamp) as "last_timestamp: i64"
        FROM metrics 
        WHERE namespace = ? 
        GROUP BY id
        ORDER BY MAX(timestamp) DESC
        LIMIT ? OFFSET ?
        "#,
        namespace,
        per_page,
        offset
    )
    .fetch_all(&*pool)
    .await;
    
    let charts = match rows {
        Ok(rows) => rows
            .into_iter()
            .map(|row| ChartInfo {
                id: row.id,
                point_count: row.point_count.unwrap_or(0),
                last_updated: row.last_timestamp
                    .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "Unknown".to_string()),
            })
            .collect::<Vec<_>>(),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    
    let template = NamespaceTemplate {
        namespace,
        charts,
        current_page: page,
        total_pages,
        has_prev: page > 1,
        has_next: page < total_pages,
    };
    
    match template.render() {
        Ok(html) => Ok(Html(html)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create database connection pool
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite:somnial.db".to_string());
    
    // Create database if it doesn't exist
    sqlx::sqlite::Sqlite::create_database(&database_url).await.ok();
    
    let pool = SqlitePool::connect(&database_url).await?;
    
    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await?;
    
    // Build application routes
    let app = Router::new()
        .route("/", get(get_index))
        .route("/favicon.svg", get(get_favicon))
        .route("/{namespace}", get(get_namespace))
        .route("/{namespace}/{id}", post(post_metric))
        .route("/{namespace}/{id}", get(get_chart))
        .with_state(pool);
    
    // Start server
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("Server running on {}", addr);
    
    axum::serve(listener, app).await?;
    
    Ok(())
}
