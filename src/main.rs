use alice::cli::{Cli, Cmd};
use alice::config::Paths;
use clap::Parser;

fn main() {
    let cli = Cli::parse();
    if let Some(p) = cli.profile.as_deref()
        && !alice::config::valid_profile_name(p) {
            eprintln!("error: invalid profile name: {p}");
            std::process::exit(2);
        }
    let paths = Paths::resolve(cli.profile.as_deref());
    let result = match cli.cmd {
        Some(Cmd::Serve) => tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(alice::serve(paths)),
        cmd => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(alice::cli::run(cmd, paths)),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
