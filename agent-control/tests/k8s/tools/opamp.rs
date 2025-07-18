use std::str::FromStr;
use url::Url;

// URL to access to services binded on ports from minikube host
// https://minikube.sigs.k8s.io/docs/handbook/host-access/
pub const MINIKUBE_HOST_ACCESS: &str = "host.minikube.internal";

pub fn get_minikube_opamp_url_from_fake_server(opamp_endpoint: &str) -> Url {
    let mut opamp_endpoint = Url::from_str(opamp_endpoint).unwrap();
    opamp_endpoint.set_host(Some(MINIKUBE_HOST_ACCESS)).unwrap();
    opamp_endpoint
}
