use sipper::sipper;

use iced::{Element, Never};

use std::borrow::Cow;
use std::path::Path;
use std::pin::Pin;

pub trait Call {
    /// Executes the call and returns its result.
    ///
    /// The returned string is fed back to the model as the tool's
    /// response. Keep tool commentary and content distinguishable with a
    /// bracket convention: wrap any message the tool itself generates —
    /// diagnostics, truncation notices, confirmations — in `[...]`, and
    /// return verbatim content (file contents, command output)
    /// unbracketed.
    fn run(&self, project: &Path) -> Run;

    fn title(&self) -> Option<Cow<'_, str>> {
        None
    }

    fn view(&self) -> Option<Element<'_, Never>> {
        None
    }
}

pub type Output = ::core::result::Result<String, reason::Error>;
pub type Run = Pin<Box<dyn sipper::Core<Output = Output, Item = String> + Send>>;

pub fn straw<F>(f: impl FnOnce(sipper::Sender<String>) -> F + Send + 'static) -> Run
where
    F: Future<Output = Result<String, reason::Error>> + Send,
{
    Box::pin(sipper(async move |sender| f(sender).await))
}

pub fn future(f: impl Future<Output = Result<String, reason::Error>> + Send + 'static) -> Run {
    Box::pin(sipper(move |_sender| f))
}
