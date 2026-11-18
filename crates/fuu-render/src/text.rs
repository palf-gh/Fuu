//! Plain-text fallback renderer: hands `SafeDocument::text` to the host UI.

use crate::{BodyRenderer, Rect, RemoteContentPolicy, RenderError, SafeDocument};

/// Receives the text to display (the host binds it to a Slint `Text`).
pub type TextSink = Box<dyn Fn(&str) + 'static>;

pub struct TextRenderer {
    sink: TextSink,
}

impl TextRenderer {
    pub fn new(sink: TextSink) -> Self {
        Self { sink }
    }
}

impl BodyRenderer for TextRenderer {
    fn name(&self) -> &'static str {
        "text"
    }

    fn show(&mut self, doc: &SafeDocument, _policy: RemoteContentPolicy) -> Result<(), RenderError> {
        (self.sink)(&doc.text);
        Ok(())
    }

    fn clear(&mut self) -> Result<(), RenderError> {
        (self.sink)("");
        Ok(())
    }

    fn set_bounds(&mut self, _rect: Rect) -> Result<(), RenderError> {
        Ok(())
    }

    fn set_visible(&mut self, _visible: bool) -> Result<(), RenderError> {
        Ok(())
    }
}
