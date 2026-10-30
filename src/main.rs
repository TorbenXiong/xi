mod app_server;
mod cli;
mod http;
mod model;
mod protocol;
mod storage;
mod util;

use anyhow::Result;
use app_server::AppServer;
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let data_dir = std::env::var_os("XI_DATA_DIR").map(PathBuf::from);
    let database = match &data_dir {
        Some(directory) => directory.join("xi.db"),
        None => storage::default_database_path()?,
    };
    let app = Arc::new(AppServer::open(&database)?);
    if args.iter().any(|arg| arg == "--cli") {
        cli::run(&app)
    } else {
        let fixed = args.iter().any(|arg| arg == "--app-server");
        http::serve(
            app,
            fixed.then_some(43127),
            fixed
                .then(|| std::env::var("XI_RUNTIME_TOKEN").ok())
                .flatten(),
        )
    }
}
