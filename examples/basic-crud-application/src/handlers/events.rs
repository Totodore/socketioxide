use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Response<T> {
    Success { data: T },
    Error { error: &'static str },
}
#[derive(Debug, Clone, Serialize)]
pub enum Error {
    NotFound,
}
impl Error {
    fn as_str(&self) -> &'static str {
        match self {
            Error::NotFound => "Not found",
        }
    }
}
impl<T> From<T> for Response<T> {
    fn from(data: T) -> Self {
        Response::Success { data }
    }
}

impl<T> From<Result<T, Error>> for Response<T> {
    fn from(result: Result<T, Error>) -> Self {
        match result {
            Ok(data) => Response::Success { data },
            Err(error) => Response::Error {
                error: error.as_str(),
            },
        }
    }
}
