//! Aggregation cloud instance id detector implementation
use thiserror::Error;

use crate::cloud::aws::detector::AWSDetector;
use crate::cloud::azure::detector::AzureDetector;
use crate::cloud::gcp::detector::GCPDetector;
use crate::cloud::http_client::{HttpClient, HttpClientUreq};
use crate::cloud::{
    AZURE_INSTANCE_ID, CLOUD_INSTANCE_ID, CLOUD_TYPE, CLOUD_TYPE_AWS, CLOUD_TYPE_AZURE,
    CLOUD_TYPE_GCP, CLOUD_TYPE_NO, GCP_INSTANCE_ID,
};
use crate::{cloud::AWS_INSTANCE_ID, Detect, DetectError, Key, Resource, Value};

/// The `AWSDetector` struct encapsulates an HTTP client used to retrieve the instance metadata.
pub struct CloudIdDetector<AWS: Detect, AZURE: Detect, GCP: Detect> {
    aws_detector: AWS,
    azure_detector: AZURE,
    gcp_detector: GCP,
}

impl Default
    for CloudIdDetector<
        AWSDetector<HttpClientUreq>,
        AzureDetector<HttpClientUreq>,
        GCPDetector<HttpClientUreq>,
    >
{
    fn default() -> Self {
        Self {
            aws_detector: AWSDetector::default(),
            azure_detector: AzureDetector::default(),
            gcp_detector: GCPDetector::default(),
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

impl<AWS, AZURE, GCP> Detect for CloudIdDetector<AWS, AZURE, GCP>
where
    AWS: Detect,
    AZURE: Detect,
    GCP: Detect,
{
    fn detect(&self) -> Result<Resource, DetectError> {
        let response = self.aws_detector.detect();

        if response.is_ok() {
            let cloud_id = response
                .expect("AWS metadata should be present at this point. Check logice")
                .get(AWS_INSTANCE_ID.into())
                .expect("AWS instance ID should be in the attributes list. Check logic.");
            return Ok(Resource::new([
                (Key::from(CLOUD_INSTANCE_ID), cloud_id),
                (
                    Key::from(CLOUD_TYPE),
                    Value::from(CLOUD_TYPE_AWS.to_string()),
                ),
            ]));
        }

        let response = self.azure_detector.detect();

        if response.is_ok() {
            let cloud_id = response
                .expect("Azure metadata should be present at this point. Check logic.")
                .get(AZURE_INSTANCE_ID.into())
                .expect("Azure instance ID should be in the attributes list. Check logic.");
            return Ok(Resource::new([
                (Key::from(CLOUD_INSTANCE_ID), cloud_id),
                (
                    Key::from(CLOUD_TYPE),
                    Value::from(CLOUD_TYPE_AZURE.to_string()),
                ),
            ]));
        }

        let response = self.gcp_detector.detect();

        if response.is_ok() {
            let cloud_id = response
                .expect("GCP metadata should be present at this point. Check logic.")
                .get(GCP_INSTANCE_ID.into())
                .expect("GCP instance ID should be in the attributes list. Check logic.");
            return Ok(Resource::new([
                (Key::from(CLOUD_INSTANCE_ID), cloud_id),
                (
                    Key::from(CLOUD_TYPE),
                    Value::from(CLOUD_TYPE_GCP.to_string()),
                ),
            ]));
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
mod test {
    use super::*;
    use crate::cloud::aws::detector::AWSDetectorError;
    use crate::cloud::azure::detector::AzureDetectorError;
    use crate::cloud::gcp::detector::GCPDetectorError;
    use crate::cloud::CLOUD_TYPE_GCP;
    use mockall::mock;

    impl<C> CloudIdDetector<AWSDetector<C>, AzureDetector<C>, GCPDetector<C>>
    where
        C: HttpClient,
    {
        fn new(aws: AWSDetector<C>, azure: AzureDetector<C>, gcp: GCPDetector<C>) -> Self {
            Self {
                aws_detector: aws,
                azure_detector: azure,
                gcp_detector: gcp,
            }
        }
    }

    mock! {
        pub DetectorMock {}
        impl Detect for DetectorMock {
            fn detect(&self) -> Result<Resource, DetectError>;
        }
    }

    #[test]
    fn detect_aws_metadata() {
        let mut aws_detector_mock = MockDetectorMock::default();
        let mut azure_detector_mock = MockDetectorMock::default();
        let mut gcp_detector_mock = MockDetectorMock::default();

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
        let mut aws_detector_mock = MockDetectorMock::default();
        let mut azure_detector_mock = MockDetectorMock::default();
        let mut gcp_detector_mock = MockDetectorMock::default();

        aws_detector_mock.expect_detect().once().returning(|| {
            Err(DetectError::AWSError(
                AWSDetectorError::UnsuccessfulResponse(404, "No VM Found".to_string()),
            ))
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
        let mut aws_detector_mock = MockDetectorMock::default();
        let mut azure_detector_mock = MockDetectorMock::default();
        let mut gcp_detector_mock = MockDetectorMock::default();

        aws_detector_mock.expect_detect().once().returning(|| {
            Err(DetectError::AWSError(
                AWSDetectorError::UnsuccessfulResponse(404, "No VM Found".to_string()),
            ))
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
        let mut aws_detector_mock = MockDetectorMock::default();
        let mut azure_detector_mock = MockDetectorMock::default();
        let mut gcp_detector_mock = MockDetectorMock::default();

        aws_detector_mock.expect_detect().once().returning(|| {
            Err(DetectError::AWSError(
                AWSDetectorError::UnsuccessfulResponse(404, "No VM Found".to_string()),
            ))
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
