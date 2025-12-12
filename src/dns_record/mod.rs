mod crd;
mod reconcile;

pub use crd::{DNSRecord, DNSRecordSpec, DNSRecordStatus};
pub use reconcile::{DOCUMENT_FINALIZER, run};
