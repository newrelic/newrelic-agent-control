//! Aggregation cloud instance id detector implementation
use crate::cloud::aws::detector::{AWSDetector, TTL_TOKEN_DEFAULT};
use crate::cloud::aws::http_client::AWSHttpClient;
use crate::cloud::azure::detector::AzureDetector;
use crate::cloud::gcp::detector::GCPDetector;
use crate::cloud::http_client::HttpClient;
use crate::cloud::{
    AZURE_INSTANCE_ID, CLOUD_INSTANCE_ID, CLOUD_TYPE, CLOUD_TYPE_AWS, CLOUD_TYPE_AZURE,
    CLOUD_TYPE_GCP, CLOUD_TYPE_NO, GCP_INSTANCE_ID,
};
use crate::{DetectError, Detector, Key, Resource, Value, cloud::AWS_INSTANCE_ID};
use thiserror::Error;
use tracing::warn;

/// The `AWSDetector` struct encapsulates an HTTP client used to retrieve the instance metadata.
pub struct CloudIdDetector<AWS: Detector, AZURE: Detector, GCP: Detector> {
    aws_detector: AWS,
    azure_detector: AZURE,
    gcp_detector: GCP,
}

impl<C> CloudIdDetector<AWSDetector<C>, AzureDetector<C>, GCPDetector<C>>
where
    C: HttpClient,
{
    /// Returns a new instance of CloudIdDetector
    pub fn new(
        azure_http_client: C,
        aws_http_client: C,
        gpc_http_client: C,
        aws_metadata_endpoint: String,
        aws_token_endpoint: String,
        azure_metadata_endpoint: String,
        gcp_metadata_endpoint: String,
    ) -> Self {
        let aws_http_client = AWSHttpClient::new(
            aws_http_client,
            aws_metadata_endpoint,
            aws_token_endpoint,
            TTL_TOKEN_DEFAULT,
        );
        Self {
            aws_detector: AWSDetector::new(aws_http_client),
            azure_detector: AzureDetector::new(azure_http_client, azure_metadata_endpoint),
            gcp_detector: GCPDetector::new(gpc_http_client, gcp_metadata_endpoint),
        }
    }
}

/// An enumeration of potential errors related to the HTTP client.
/// // TODO: should be updated to the scope
#[derive(Error, Debug)]
pub enum CloudIdDetectorError {
    /// Unsuccessful cloud detection.
    #[error("Non of cloud API responded")]
    UnsuccessfulCloudIdCheck(),
}

fn match_resource(
    resource: Resource,
    cloud_instance_id_const: &str,
    cloud_type_const: &str,
) -> Resource {
    match resource.get(cloud_instance_id_const.into()) {
        None => {
            warn!(
                "{} instance ID should be in the attributes list. Check API permissions",
                cloud_type_const
            );
            Resource::new([
                (Key::from(CLOUD_INSTANCE_ID), Value::from("".to_string())),
                (
                    Key::from(CLOUD_TYPE),
                    Value::from(CLOUD_TYPE_NO.to_string()),
                ),
            ])
        }
        Some(cloud_id) => Resource::new([
            (Key::from(CLOUD_INSTANCE_ID), cloud_id),
            (
                Key::from(CLOUD_TYPE),
                Value::from(cloud_type_const.to_string()),
            ),
        ]),
    }
}

impl<AWS, AZURE, GCP> Detector for CloudIdDetector<AWS, AZURE, GCP>
where
    AWS: Detector,
    AZURE: Detector,
    GCP: Detector,
{
    fn detect(&self) -> Result<Resource, DetectError> {
        if let Ok(resource) = self.aws_detector.detect() {
            return Ok(match_resource(resource, AWS_INSTANCE_ID, CLOUD_TYPE_AWS));
        }

        if let Ok(resource) = self.azure_detector.detect() {
            return Ok(match_resource(
                resource,
                AZURE_INSTANCE_ID,
                CLOUD_TYPE_AZURE,
            ));
        }

        if let Ok(resource) = self.gcp_detector.detect() {
            return Ok(match_resource(resource, GCP_INSTANCE_ID, CLOUD_TYPE_GCP));
        }

        Ok(Resource::new([
            (Key::from(CLOUD_INSTANCE_ID), Value::from("".to_string())),
            (
                Key::from(CLOUD_TYPE),
                Value::from(CLOUD_TYPE_NO.to_string()),
            ),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::CLOUD_TYPE_GCP;
    use crate::cloud::aws::detector::AWSDetectorError;
    use crate::cloud::azure::detector::AzureDetectorError;
    use crate::cloud::gcp::detector::GCPDetectorError;
    use crate::cloud::http_client::HttpClientError;
    use mockall::mock;

    mock! {
        pub Detector {}
        impl Detector for Detector {
            fn detect(&self) -> Result<Resource, DetectError>;
        }
    }

    #[test]
    fn detect_aws_metadata() {
        let mut aws_detector_mock = MockDetector::default();
        let azure_detector_mock = MockDetector::default();
        let gcp_detector_mock = MockDetector::default();

        aws_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource::new([(
                Key::from(AWS_INSTANCE_ID),
                Value::from("i-1234567890abcdef0".to_string()),
            )]))
        });

        let detector = CloudIdDetector {
            aws_detector: aws_detector_mock,
            azure_detector: azure_detector_mock,
            gcp_detector: gcp_detector_mock,
        };

        let identifiers = detector.detect().unwrap();

        assert_eq!(
            "i-1234567890abcdef0".to_string(),
            String::from(identifiers.get(CLOUD_INSTANCE_ID.into()).unwrap())
        );

        assert_eq!(
            CLOUD_TYPE_AWS.to_string(),
            String::from(identifiers.get(CLOUD_TYPE.into()).unwrap())
        );
    }

    #[test]
    fn detect_azure_metadata() {
        let mut aws_detector_mock = MockDetector::default();
        let mut azure_detector_mock = MockDetector::default();
        let gcp_detector_mock = MockDetector::default();

        aws_detector_mock.expect_detect().once().returning(|| {
            Err(DetectError::AWSError(AWSDetectorError::HttpError(
                HttpClientError::ResponseError(404, "No VM Found".to_string()),
            )))
        });

        azure_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource::new([(
                Key::from(AZURE_INSTANCE_ID),
                Value::from("02aab8a4-74ef-476e-8182-f6d2ba4166a6".to_string()),
            )]))
        });

        let detector = CloudIdDetector {
            aws_detector: aws_detector_mock,
            azure_detector: azure_detector_mock,
            gcp_detector: gcp_detector_mock,
        };

        let identifiers = detector.detect().unwrap();

        assert_eq!(
            "02aab8a4-74ef-476e-8182-f6d2ba4166a6".to_string(),
            String::from(identifiers.get(CLOUD_INSTANCE_ID.into()).unwrap())
        );

        assert_eq!(
            CLOUD_TYPE_AZURE.to_string(),
            String::from(identifiers.get(CLOUD_TYPE.into()).unwrap())
        );
    }

    #[test]
    fn detect_gcp_metadata() {
        let mut aws_detector_mock = MockDetector::default();
        let mut azure_detector_mock = MockDetector::default();
        let mut gcp_detector_mock = MockDetector::default();

        aws_detector_mock.expect_detect().once().returning(|| {
            Err(DetectError::AWSError(AWSDetectorError::HttpError(
                HttpClientError::ResponseError(404, "No VM Found".to_string()),
            )))
        });

        azure_detector_mock.expect_detect().once().returning(|| {
            Err(DetectError::AzureError(
                AzureDetectorError::UnsuccessfulResponse(404, "No VM Found".to_string()),
            ))
        });

        gcp_detector_mock.expect_detect().once().returning(|| {
            Ok(Resource::new([(
                Key::from(GCP_INSTANCE_ID),
                Value::from("6331980990053453154".to_string()),
            )]))
        });

        let detector = CloudIdDetector {
            aws_detector: aws_detector_mock,
            azure_detector: azure_detector_mock,
            gcp_detector: gcp_detector_mock,
        };

        let identifiers = detector.detect().unwrap();

        assert_eq!(
            "6331980990053453154".to_string(),
            String::from(identifiers.get(CLOUD_INSTANCE_ID.into()).unwrap())
        );

        assert_eq!(
            CLOUD_TYPE_GCP.to_string(),
            String::from(identifiers.get(CLOUD_TYPE.into()).unwrap())
        );
    }

    #[test]
    fn detect_nothing_metadata() {
        let mut aws_detector_mock = MockDetector::default();
        let mut azure_detector_mock = MockDetector::default();
        let mut gcp_detector_mock = MockDetector::default();

        aws_detector_mock.expect_detect().once().returning(|| {
            Err(DetectError::AWSError(AWSDetectorError::HttpError(
                HttpClientError::ResponseError(404, "No VM Found".to_string()),
            )))
        });

        azure_detector_mock.expect_detect().once().returning(|| {
            Err(DetectError::AzureError(
                AzureDetectorError::UnsuccessfulResponse(404, "No VM Found".to_string()),
            ))
        });

        gcp_detector_mock.expect_detect().once().returning(|| {
            Err(DetectError::GCPError(
                GCPDetectorError::UnsuccessfulResponse(404, "No VM Found".to_string()),
            ))
        });

        let detector = CloudIdDetector {
            aws_detector: aws_detector_mock,
            azure_detector: azure_detector_mock,
            gcp_detector: gcp_detector_mock,
        };

        let identifiers = detector.detect().unwrap();

        assert_eq!(
            "".to_string(),
            String::from(identifiers.get(CLOUD_INSTANCE_ID.into()).unwrap())
        );

        assert_eq!(
            CLOUD_TYPE_NO.to_string(),
            String::from(identifiers.get(CLOUD_TYPE.into()).unwrap())
        );
    }
}
