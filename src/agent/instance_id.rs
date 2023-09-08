use ulid::Ulid;

pub trait InstanceIDGetter {
    fn get(&self, name:String) -> String;
}

#[derive(Default)]
pub struct ULIDInstanceIDGetter {}

impl InstanceIDGetter for ULIDInstanceIDGetter {
    fn get(&self, _:String) -> String {
        Ulid::new().to_string()
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use mockall::mock;

    mock! {
        pub InstanceIDGetterMock {}

        impl InstanceIDGetter for InstanceIDGetterMock {
            fn get(&self, name:String) -> String;
        }
    }
}