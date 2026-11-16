//! What importing and reporting need of a store: the intake core that
//! files a message and holds a reported template. The flows themselves
//! are `mailrs_intake::import` and `mailrs_intake::scam_report`.

use std::sync::Arc;

use mailrs_intake::scam_report::ReportStore;
use mailrs_intake::sent::SentStore;

pub trait IntakeStore: Send + Sync + 'static {
    type Core: SentStore + ReportStore + Send + 'static;

    /// Built on a runtime thread and moved into the blocking task that
    /// runs the flow: a core may take handles that only a runtime thread
    /// can give.
    fn intake_core(self: &Arc<Self>) -> Self::Core;
}
