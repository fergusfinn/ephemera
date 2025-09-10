use askama::Template;
use axum::{
    body::Body,
    extract::{Path, Query},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use chrono::Utc;
// Using resvg for high-quality SVG to PNG rendering
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
        .body(Body::from(FAVICON_SVG.to_string()))
        .unwrap()
}

fn generate_sparkline_badge(data: &[MetricPoint], metric_name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Badge dimensions
    let width = 240;
    let height = 40;
    let corner_radius = 6;
    let padding = 8;
    
    // Generate sparkline path if we have data
    let sparkline_path = if !data.is_empty() {
        let values: Vec<f64> = data.iter().map(|p| p.value).collect();
        let min_val = values.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        let max_val = values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        
        let chart_width = width - 2 * padding;
        let chart_height = 16; // Space for sparkline
        let chart_y_start = 20; // Below text
        
        let mut path_data = String::new();
        
        if data.len() == 1 {
            // Single point - draw a small horizontal line
            let x = width / 2;
            let y = chart_y_start + chart_height / 2;
            path_data.push_str(&format!("M{} {} L{} {}", x - 5, y, x + 5, y));
        } else if max_val == min_val {
            // Flat line - all values the same
            let y = chart_y_start + chart_height / 2;
            path_data.push_str(&format!("M{} {} L{} {}", padding, y, width - padding, y));
        } else {
            // Normal sparkline with varying values - use timestamp-based X positioning
            let timestamps: Vec<i64> = data.iter().map(|p| p.timestamp).collect();
            let min_time = *timestamps.iter().min().unwrap();
            let max_time = *timestamps.iter().max().unwrap();
            let time_range = (max_time - min_time).max(1); // Avoid division by zero
            
            for (i, point) in data.iter().enumerate() {
                let x = if time_range > 0 {
                    padding + ((point.timestamp - min_time) as f64 / time_range as f64 * chart_width as f64) as i32
                } else {
                    padding + (i as i32 * chart_width / (data.len() - 1) as i32)
                };
                let y = chart_y_start + chart_height - ((point.value - min_val) / (max_val - min_val) * chart_height as f64) as i32;
                
                if i == 0 {
                    path_data.push_str(&format!("M{} {}", x, y));
                } else {
                    path_data.push_str(&format!(" L{} {}", x, y));
                }
            }
        }
        
        path_data
    } else {
        String::new()
    };
    
    // Generate SVG
    let svg = format!(
        r#"<svg width="{}" height="{}" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <style>
      .badge-bg {{ fill: white; stroke: black; stroke-width: 1; }}
      .badge-text {{ font-family: monospace; font-size: 11px; fill: black; font-weight: bold; }}
      .sparkline {{ fill: none; stroke: black; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }}
    </style>
  </defs>
  
  <!-- White background -->
  <rect x="0" y="0" width="{}" height="{}" fill="white"/>
  
  <!-- Background rounded rectangle with border -->
  <rect x="0.5" y="0.5" width="{}" height="{}" rx="{}" ry="{}" class="badge-bg"/>
  
  <!-- Metric name -->
  <text x="{}" y="13" class="badge-text">{}</text>
  
  <!-- Sparkline -->
  {}
  
</svg>"#,
        width, height,
        width, height,
        width - 1, height - 1, corner_radius, corner_radius,
        padding, 
        escape_xml(metric_name),
        if sparkline_path.is_empty() {
            String::new()
        } else {
            format!(r#"<path d="{}" class="sparkline"/>"#, sparkline_path)
        }
    );
    
    // Parse SVG and render to PNG
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_str(&svg, &opt)?;
    
    let pixmap_size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height())
        .ok_or("Failed to create pixmap")?;
    
    resvg::render(&tree, usvg::Transform::default(), &mut pixmap.as_mut());
    
    // Convert to PNG
    let png_data = pixmap.encode_png()?;
    Ok(png_data)
}

fn escape_xml(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '&' => "&amp;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#39;".to_string(),
            _ => c.to_string(),
        })
        .collect()
}


async fn get_badge(
    Path((namespace, id)): Path<(String, String)>,
    pool: axum::extract::State<SqlitePool>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    // Get last 50 points for the sparkline
    let rows = sqlx::query!(
        "SELECT value, timestamp FROM metrics WHERE namespace = ? AND id = ? ORDER BY timestamp DESC LIMIT 50",
        namespace,
        id
    )
    .fetch_all(&*pool)
    .await;
    
    let mut data = match rows {
        Ok(rows) => rows
            .into_iter()
            .map(|row| MetricPoint {
                timestamp: row.timestamp,
                value: row.value,
            })
            .collect::<Vec<_>>(),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    
    // Reverse to get chronological order
    data.reverse();
    
    // Generate ETag based on latest timestamp and data count
    let etag = if let Some(latest) = data.first() {
        format!("\"{}:{}\"", latest.timestamp, data.len())
    } else {
        "\"empty\"".to_string()
    };
    
    // Check if client has current version
    if let Some(if_none_match) = headers.get("if-none-match") {
        if if_none_match.to_str().unwrap_or("") == etag {
            return Ok(Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header("etag", &etag)
                .header("cache-control", "public, max-age=300")
                .body(Body::empty())
                .unwrap());
        }
    }
    
    match generate_sparkline_badge(&data, &id) {
        Ok(png_data) => {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "image/png")
                .header("etag", etag)
                .header("cache-control", "public, max-age=300")
                .body(Body::from(png_data))
                .unwrap())
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
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
        .route("/{namespace}/{id}/badge.png", get(get_badge))
        .with_state(pool);
    
    // Start server
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("Server running on {}", addr);
    
    axum::serve(listener, app).await?;
    
    Ok(())
}
