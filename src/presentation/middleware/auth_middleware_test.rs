// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#[cfg(test)]
mod tests {
    use crate::config::settings::Settings;
    use crate::infrastructure::database::connection;
    use crate::presentation::middleware::auth_middleware::{auth_middleware, AuthState};
    use axum::{
        body::Body,
        http::{Request, StatusCode, HeaderMap, HeaderValue},
        middleware,
        routing::get,
        Router,
    };
    use sea_orm::{Database, DatabaseConnection, ConnectionTrait, DbBackend, Statement};
    use tower::ServiceExt;
    use uuid::Uuid;

    async fn setup_app_with_db() -> (Router, DatabaseConnection) {
        // Create in-memory SQLite database for testing
        let db = Database::connect("sqlite::memory:").await.unwrap();
        
        // Create test team and API key
        let team_id = Uuid::new_v4();
        let api_key = Uuid::new_v4().to_string();
        
        // Insert test data using SQLite syntax
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, 'test-team', datetime('now'), datetime('now'))",
            vec![team_id.into()],
        ))
        .await
        .unwrap();

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO api_keys (id, key, team_id, created_at, updated_at) VALUES (?, ?, ?, datetime('now'), datetime('now'))",
            vec![Uuid::new_v4().into(), api_key.clone().into(), team_id.into()],
        ))
        .await
        .unwrap();

        let auth_state = AuthState {
            db: db.clone(),
            team_id: Uuid::nil(), // Will be set by middleware
        };

        let app = Router::new()
            .route("/", get(|| async { "Hello" }))
            .route("/protected", get(|| async { "Protected" }))
            .layer(middleware::from_fn_with_state(auth_state, auth_middleware));

        (app, db)
    }

    #[tokio::test]
    async fn test_auth_middleware_missing_header() {
        let (app, _db) = setup_app_with_db().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_middleware_invalid_header() {
        let (app, _db) = setup_app_with_db().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", "Bearer invalid-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_middleware_valid_header() {
        let (app, db) = setup_app_with_db().await;
        
        // Get the API key we created
        let api_key = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT key FROM api_keys LIMIT 1",
                vec![],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<String>("key")
            .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", format!("Bearer {}", api_key))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
