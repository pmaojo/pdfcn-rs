use std::fmt;

#[derive(Debug)]
pub enum CoreError {
    Parse(pdfcn_parser::ParseError),
    Eval(pdfcn_template::EvalError),
    Json(serde_json::Error),
    Yaml(serde_yaml::Error),
    Render(String),
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::Parse(e) => write!(f, "template parse error: {e}"),
            CoreError::Eval(e) => write!(f, "template evaluation error: {e}"),
            CoreError::Json(e) => write!(f, "invalid JSON data: {e}"),
            CoreError::Yaml(e) => write!(f, "invalid YAML data: {e}"),
            CoreError::Render(e) => write!(f, "PDF rendering failed: {e}"),
        }
    }
}

impl std::error::Error for CoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CoreError::Parse(e) => Some(e),
            CoreError::Eval(e) => Some(e),
            CoreError::Json(e) => Some(e),
            CoreError::Yaml(e) => Some(e),
            CoreError::Render(_) => None,
        }
    }
}

impl From<pdfcn_parser::ParseError> for CoreError {
    fn from(e: pdfcn_parser::ParseError) -> Self {
        CoreError::Parse(e)
    }
}

impl From<pdfcn_template::EvalError> for CoreError {
    fn from(e: pdfcn_template::EvalError) -> Self {
        CoreError::Eval(e)
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(e: serde_json::Error) -> Self {
        CoreError::Json(e)
    }
}

impl From<serde_yaml::Error> for CoreError {
    fn from(e: serde_yaml::Error) -> Self {
        CoreError::Yaml(e)
    }
}
