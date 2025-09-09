use axum::{
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;

#[derive(Deserialize)]
struct PostMetricQuery {
    value: f64,
}

#[derive(Serialize)]
struct MetricPoint {
    timestamp: i64,
    value: f64,
}

async fn post_metric(
    Path((namespace, id)): Path<(String, String)>,
    Query(params): Query<PostMetricQuery>,
    headers: HeaderMap,
    pool: axum::extract::State<SqlitePool>,
) -> Result<impl IntoResponse, StatusCode> {
    let timestamp = Utc::now().timestamp();
    let owner_token = headers.get("owner-token")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    
    // Check if namespace exists and if we have permission
    if let Some(ref token) = owner_token {
        let existing = sqlx::query!(
            "SELECT owner_token FROM metrics WHERE namespace = ? LIMIT 1",
            namespace
        )
        .fetch_optional(&*pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        
        if let Some(existing_row) = existing {
            if let Some(existing_token) = existing_row.owner_token {
                if existing_token != *token {
                    return Err(StatusCode::FORBIDDEN);
                }
            }
        }
    }
    
    let result = sqlx::query!(
        "INSERT INTO metrics (namespace, id, value, timestamp, owner_token) VALUES (?, ?, ?, ?, ?)",
        namespace,
        id,
        params.value,
        timestamp,
        owner_token
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
    
    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Metrics Chart - {}</title>
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <style>
        body {{ font-family: Arial, sans-serif; margin: 20px; }}
        .container {{ max-width: 800px; margin: 0 auto; }}
        h1 {{ color: #333; }}
        .chart-container {{ position: relative; height: 400px; margin: 20px 0; }}
    </style>
</head>
<body>
    <div class="container">
        <h1>Metrics for: {}/{}</h1>
        <div class="chart-container">
            <canvas id="chart"></canvas>
        </div>
    </div>
    <script>
        const data = {};
        const ctx = document.getElementById('chart').getContext('2d');
        new Chart(ctx, {{
            type: 'line',
            data: {{
                labels: data.map(point => new Date(point.timestamp * 1000).toLocaleString()),
                datasets: [{{
                    label: '{}',
                    data: data.map(point => point.value),
                    borderColor: 'rgb(75, 192, 192)',
                    backgroundColor: 'rgba(75, 192, 192, 0.2)',
                    tension: 0.1
                }}]
            }},
            options: {{
                responsive: true,
                maintainAspectRatio: false,
                scales: {{
                    y: {{
                        beginAtZero: false
                    }}
                }}
            }}
        }});
    </script>
</body>
</html>"#,
        format!("{}/{}", namespace, id), namespace, id, data_json, format!("{}/{}", namespace, id)
    );
    
    Ok(Html(html))
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create database connection pool
    let database_url = "sqlite:metrics.db";
    let pool = SqlitePool::connect(database_url).await?;
    
    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await?;
    
    // Build application routes
    let app = Router::new()
        .route("/charts/{namespace}/{id}", post(post_metric))
        .route("/charts/{namespace}/{id}", get(get_chart))
        .with_state(pool);
    
    // Start server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("Server running on http://localhost:3000");
    
    axum::serve(listener, app).await?;
    
    Ok(())
}
