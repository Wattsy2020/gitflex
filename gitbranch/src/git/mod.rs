mod history;
mod rebase_history;
mod repository;
mod switch_history;
pub use repository::{
    Checkout, CleanBranch, ConflictableCommandOutcome, Error, HeadOperationRepository, LocalBranch,
    Repository,
};
pub use switch_history::SwitchHistory;
