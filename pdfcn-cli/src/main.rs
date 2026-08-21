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
    Build {
        /// Path to the .haml template
        template: PathBuf,
        /// Path to the JSON/YAML data file
        #[arg(short, long)]
        data: PathBuf,
        /// Output PDF path
        #[arg(short, long, default_value = "out.pdf")]
        out: PathBuf,
        /// Page size: a4, letter, or "<width>x<height>" in mm
        #[arg(long, default_value = "a4")]
        size: String,
        /// portrait or landscape
        #[arg(long, default_value = "portrait")]
        orientation: String,
        /// Page margin in millimeters
        #[arg(long, default_value_t = 10.0)]
        margin: f32,
        /// Fetch `<img src="http(s)://...">` sources over the network and
        /// embed them (opt-in: pdfcn never does this by default, see NFR-3)
        #[arg(long, default_value_t = false)]
        fetch_remote_images: bool,
    },
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
        Command::Build {
            template,
            data,
            out,
            size,
            orientation,
            margin,
            fetch_remote_images,
        } => build::run(
            &template,
            &data,
            &out,
            &size,
            &orientation,
            margin,
            fetch_remote_images,
        ),
        Command::Dev {
            template,
            data,
            port,
        } => dev::run(&template, &data, port),
    }
}
