#[cfg(test)]
mod tests_mocked_executor {
    use crate::k8s::error::K8sError::Generic;
    use kube::error::ErrorResponse;
    use kube::Error;

    ///
    /// The following tests are just an example to show how the K8sExecutor con be mocked completely
    ///

    #[double]
    use crate::k8s::executor::K8sExecutor;
    use mockall_double::double;

    #[tokio::test]
    async fn test_version_with_whole_mock() {
        let mut mock = K8sExecutor::default();
        mock.expect_get_minor_version()
            .returning(|| Ok("24".to_string()));

        let version = mock.get_minor_version().await;
        assert_eq!(true, version.is_ok());
        let version = version.unwrap();
        assert_eq!(version, "24");
    }

    #[tokio::test]
    async fn test_get_pods_with_whole_mock() {
        let mut mock = K8sExecutor::default();
        mock.expect_get_pods().returning(|| {
            Err(Generic(Error::Api(ErrorResponse {
                status: "test".to_string(),
                message: "test".to_string(),
                reason: "test".to_string(),
                code: 404,
            })))
        });

        let list_pods = mock.get_pods().await;
        assert_eq!(true, list_pods.is_err());
    }
}
