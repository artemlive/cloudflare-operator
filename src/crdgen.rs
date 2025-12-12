use controller::dns_record::DNSRecord;
use kube::CustomResourceExt;
fn main() {
    print!("{}", serde_yaml::to_string(&DNSRecord::crd()).unwrap())
}
