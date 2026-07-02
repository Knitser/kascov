mod wrpc;

pub use wrpc::{compute_covenant_id, NodeHandle};

use crate::model::*;
use crate::Result;

/// Read access to the chain, as the sync engine needs it. Implemented by the
/// live wRPC client and by in-memory fakes in tests.
pub trait ChainSource {
    fn dag_info(&self) -> impl std::future::Future<Output = Result<DagInfo>>;
    fn block_with_txs(&self, hash: BlockHash) -> impl std::future::Future<Output = Result<Block>>;
    fn virtual_chain_from(&self, cursor: BlockHash) -> impl std::future::Future<Output = Result<ChainStep>>;
}

impl ChainSource for NodeHandle {
    async fn dag_info(&self) -> Result<DagInfo> {
        NodeHandle::dag_info(self).await
    }
    async fn block_with_txs(&self, hash: BlockHash) -> Result<Block> {
        NodeHandle::block_with_txs(self, hash).await
    }
    async fn virtual_chain_from(&self, cursor: BlockHash) -> Result<ChainStep> {
        NodeHandle::virtual_chain_from(self, cursor).await
    }
}
