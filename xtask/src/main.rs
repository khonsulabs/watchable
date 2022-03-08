use khonsu_tools::{
    publish,
    universal::{anyhow, audit, clap::Parser, DefaultConfig},
};

fn main() -> anyhow::Result<()> {
    khonsu_tools::Commands::parse().execute::<Config>()
}

enum Config {}

impl khonsu_tools::Config for Config {
    type Publish = Self;
    type Universal = Self;
}

impl khonsu_tools::universal::Config for Config {
    type Audit = Self;
    type CodeCoverage = DefaultConfig;
}

impl audit::Config for Config {
    fn args() -> Vec<String> {
        vec![
            String::from("--all-features"),
            String::from("--exclude=xtask"),
            String::from("--exclude=benchmarks"),
            // examples that include other dependencies, which aren't actually
            // indicative of the security of BonsaiDb.
            String::from("--exclude=axum"),
            String::from("--exclude=acme"),
            String::from("--exclude=view-histogram"),
        ]
    }
}

impl publish::Config for Config {
    fn paths() -> Vec<String> {
        vec![
            String::from("crates/bonsaidb-macros"),
            String::from("crates/bonsaidb-core"),
            String::from("crates/bonsaidb-utils"),
            String::from("crates/bonsaidb-local"),
            String::from("crates/bonsaidb-server"),
            String::from("crates/bonsaidb-client"),
            String::from("crates/bonsaidb-keystorage-s3"),
            String::from("crates/bonsaidb"),
        ]
    }
}
