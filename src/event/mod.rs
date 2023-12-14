pub mod channel;
#[allow(clippy::module_inception)]
pub mod event;

pub trait Consumer<E> {
    fn consume(&self) -> E;
}

pub trait Publisher<E> {
    fn publish(&self, event: E);
}
