pub mod channel;
#[allow(clippy::module_inception)]
pub mod event;

pub trait EventConsumer<E>
where
    E: Send + Sync,
{
    fn consume(&self) -> E;
}

pub trait EventPublisher<E>: Clone + Send + Sync
where
    E: Send + Sync,
{
    fn publish(&self, event: E);
}
