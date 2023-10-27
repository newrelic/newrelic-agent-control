use ulid::Ulid;

pub trait InstanceIDGetter {
    // TODO: does name need to be owned?
    fn get(&self, name: String) -> String;
}

#[derive(Default)]
pub struct ULIDInstanceIDGetter {}

impl InstanceIDGetter for ULIDInstanceIDGetter {
    fn get(&self, _: String) -> String {
        Ulid::new().to_string()
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use mockall::{mock, predicate};

    mock! {
        pub InstanceIDGetterMock {}

        impl InstanceIDGetter for InstanceIDGetterMock {
            fn get(&self, name:String) -> String;
        }
    }

    impl MockInstanceIDGetterMock {
        pub fn should_get(&mut self, name: String, instance_id: String) {
            self.expect_get()
                .once()
                .with(predicate::eq(name))
                .returning(move |_| instance_id.clone());
        }
    }
}
