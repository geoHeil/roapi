use std::collections::HashMap;
use std::sync::Arc;

use columnq::table::TableSource;
use log::info;
use tokio::sync::{Mutex, RwLock};

use crate::config::Config;
use crate::context::ConcurrentRoapiContext;
use crate::context::RawRoapiContext;
use crate::server;

pub struct Application {
    http_addr: std::net::SocketAddr,
    http_server: server::http::HttpApiServer,
    postgres_server: Box<dyn server::RunnableServer>,
}

impl Application {
    pub async fn build(config: Config) -> anyhow::Result<Self> {
        let default_host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());

        let handler_ctx = RawRoapiContext::new(&config)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        let tables = config
            .tables
            .iter()
            .map(|t| (t.name.clone(), t.clone()))
            .collect::<HashMap<String, TableSource>>();
        let tables = Arc::new(Mutex::new(tables));

        if config.disable_read_only {
            let ctx_ext = Arc::new(RwLock::new(handler_ctx));
            let postgres_server = Box::new(
                server::postgres::PostgresServer::new(
                    ctx_ext.clone(),
                    &config,
                    default_host.clone(),
                )
                .await,
            );
            let (http_server, http_addr) = server::http::build_http_server::<ConcurrentRoapiContext>(
                ctx_ext,
                tables,
                &config,
                default_host,
            )?;

            Ok(Self {
                http_addr,
                http_server,
                postgres_server,
            })
        } else {
            let ctx_ext = Arc::new(handler_ctx);
            let postgres_server = Box::new(
                server::postgres::PostgresServer::new(
                    ctx_ext.clone(),
                    &config,
                    default_host.clone(),
                )
                .await,
            );
            let (http_server, http_addr) = server::http::build_http_server::<RawRoapiContext>(
                ctx_ext,
                tables,
                &config,
                default_host,
            )?;

            Ok(Self {
                http_addr,
                http_server,
                postgres_server,
            })
        }
    }

    pub fn http_addr(&self) -> std::net::SocketAddr {
        self.http_addr
    }

    pub fn postgres_addr(&self) -> std::net::SocketAddr {
        self.postgres_server.addr()
    }

    pub async fn run_until_stopped(self) -> anyhow::Result<()> {
        let postgres_server = self.postgres_server;
        info!(
            "🚀 Listening on {} for Postgres traffic...",
            postgres_server.addr()
        );
        tokio::spawn(async move {
            postgres_server
                .run()
                .await
                .expect("Failed to run postgres server");
        });

        info!("🚀 Listening on {} for HTTP traffic...", self.http_addr);
        Ok(self.http_server.await?)
    }
}
