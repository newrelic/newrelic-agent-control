// These tests leverages an in-cluster chart repository populated with fixed versions which consist in the latest
// released chart with a changed version.
// The AC image corresponds to the compiled from the current code. Tilt is used to orchestrate all these
// test environment set-up.

pub const LOCAL_CHART_REPOSITORY: &str = "http://chartmuseum.default.svc.cluster.local:8080";

// # - 0.0.0-latest-released uses remote image
// # - 0.0.1-dev and 0.0.2-dev the image tilt.local/ac-dev:dev
// # - 0.0.0-crash use an image executing exit 1

// This version contains the image from remote
pub const CHART_VERSION_LATEST_RELEASE: &str = "0.0.0-latest-released";
// This version contains the compiled dev image
pub const CHART_VERSION_DEV_1: &str = "0.0.1-dev";
// This version contains the compiled dev image
pub const CHART_VERSION_DEV_2: &str = "0.0.2-dev";
// This version contains image failing with exit 1
pub const CHART_VERSION_CRASHLOOP: &str = "0.0.0-crash";

pub const MISSING_VERSION: &str = "9.9.9";
