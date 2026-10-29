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
    // 只读导入客户端原型数据库；显式测试目录不会访问真实配置。
    let legacy = data_dir
        .unwrap_or_else(|| PathBuf::from("config"))
        .join("xi.sqlite3");
    app.import_client_database(&legacy)?;
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
