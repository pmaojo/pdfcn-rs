mod add;
mod build;
mod dev;
mod new;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "pdfcn", about = "HAML-like templates to PDF, Vercel-safe")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold a new template project with a mock data file
    New {
        /// Template/project name
        template: String,
        /// Directory to create it in (default: `./<template>`)
        #[arg(short, long)]
        dir: Option<PathBuf>,
    },
    /// Copy a preset component's HAML into ./templates/components/
    Add {
        /// Component name (Card, Badge, InvoiceTable, ...)
        component: String,
    },
    /// Compile a template + data file straight to a PDF
    Build(Box<build::BuildArgs>),
    /// Serve a live preview of the rendered document in a browser
    Dev {
        /// Path to the .haml template
        template: PathBuf,
        /// Path to the JSON/YAML data file
        #[arg(short, long)]
        data: PathBuf,
        /// Port to listen on
        #[arg(short, long, default_value_t = 4321)]
        port: u16,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::New { template, dir } => new::run(&template, dir.as_deref()),
        Command::Add { component } => add::run(&component),
        Command::Build(args) => build::run(*args),
        Command::Dev {
            template,
            data,
            port,
        } => dev::run(&template, &data, port),
    }
}
