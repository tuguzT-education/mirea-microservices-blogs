#![cfg(test)]

use std::{
    net::{SocketAddr, TcpListener},
    sync::Arc,
};

use anyhow::Context;
use axum::{Router, Server};
use diesel::{
    r2d2::{ConnectionManager, Pool},
    PgConnection,
};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use reqwest::{Client, StatusCode};
use uuid::Uuid;

use crate::{
    data::{
        model::{Blog, CreateBlog},
        repository::{DynBlogRepository, LocalBlogRepository},
    },
    route::{blog, health},
};

#[tokio::test]
async fn test_health() {
    let addr = "127.0.0.1:8080".parse::<SocketAddr>().unwrap();
    let listener = TcpListener::bind(addr).unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let app = Router::new().merge(health::health());
        Server::from_tcp(listener)
            .unwrap()
            .serve(app.into_make_service())
            .await
            .unwrap();
    });

    let client = Client::new();
    let response = client
        .get(format!("http://{}/health", addr))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "Healthy");
}

#[tokio::test]
async fn test_create() {
    let addr = "127.0.0.1:8081".parse::<SocketAddr>().unwrap();
    let listener = TcpListener::bind(addr).unwrap();
    let addr = listener.local_addr().unwrap();

    let user_id = Uuid::new_v4();
    let name = "New blog";

    tokio::spawn(async move {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let manager = ConnectionManager::<PgConnection>::new(database_url);
        let pool = Pool::builder()
            .test_on_check_out(true)
            .build(manager)
            .with_context(|| "failed to establish connection to database")
            .unwrap();
        const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
        let connection = &mut pool.get().unwrap();
        connection.run_pending_migrations(MIGRATIONS).unwrap();

        let blog_repo = Arc::new(LocalBlogRepository::new(pool)) as DynBlogRepository;
        let new_blog = CreateBlog {
            user_id,
            name: name.to_string(),
        };
        blog_repo.create_one(new_blog.clone()).await.unwrap();

        let app = Router::new()
            .merge(blog::all_merged())
            .with_state(blog_repo)
            .merge(health::health());

        Server::from_tcp(listener)
            .unwrap()
            .serve(app.into_make_service())
            .await
            .unwrap()
    });

    let client = Client::new();
    let response = client
        .get(format!("http://{}/blog/all", addr))
        .send()
        .await
        .unwrap();
    let blogs: Vec<Blog> = response.json().await.unwrap();
    let blog = blogs
        .into_iter()
        .find(|blog| blog.user_id == user_id)
        .with_context(|| "at least 1 blog should present")
        .unwrap();

    assert_eq!(blog.name, name);
}
