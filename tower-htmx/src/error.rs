#[derive(Debug)]
pub enum Error<F, B> {
    Future(F),
    Body(B),
    HTTP(http::Error),
    Recursion,
}

impl<F, B> Error<F, B> {
    /// Format the given error as HTML.
    pub fn to_html(self) -> String {
        match self {
            Error::Future(err) => "future error".to_owned(),
            Error::Body(err) => "body error".to_owned(),
            Error::HTTP(err) => "http error".to_owned(),
            Error::Recursion => "recursion error".to_owned(),
        }
    }
}
