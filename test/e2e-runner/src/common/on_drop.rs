/// Utility to run actions whenever the test are finished even on panic.
pub struct CleanUp<F: FnOnce()> {
    action: Option<F>,
}
impl<F: FnOnce()> CleanUp<F> {
    pub fn new(action: F) -> Self {
        Self {
            action: Some(action),
        }
    }
}
impl<F: FnOnce()> Drop for CleanUp<F> {
    fn drop(&mut self) {
        if let Some(action) = self.action.take() {
            action();
        }
    }
}
