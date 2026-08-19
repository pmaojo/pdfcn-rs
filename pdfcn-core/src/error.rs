#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("template parse error: {0}")]
    Parse(#[from] pdfcn_parser::ParseError),
    #[error("template evaluation error: {0}")]
    Eval(#[from] pdfcn_template::EvalError),
    #[error("invalid JSON data: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid YAML data: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("PDF rendering failed: {0}")]
    Render(String),
}
