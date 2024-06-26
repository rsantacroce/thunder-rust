//! RPC API

use std::net::SocketAddr;

use bip300301::bitcoin;
use jsonrpsee::{core::RpcResult, proc_macros::rpc};
use thunder::types::{Address, Txid};

#[rpc(client, server)]
pub trait Rpc {
    /// Get balance in sats
    #[method(name = "balance")]
    async fn balance(&self) -> RpcResult<u64>;

    /// Connect to a peer
    #[method(name = "connect_peer")]
    async fn connect_peer(&self, addr: SocketAddr) -> RpcResult<()>;

    /// Format a deposit address
    #[method(name = "format_deposit_address")]
    async fn format_deposit_address(
        &self,
        address: Address,
    ) -> RpcResult<String>;

    /// Generate a mnemonic seed phrase
    #[method(name = "generate_mnemonic")]
    async fn generate_mnemonic(&self) -> RpcResult<String>;

    /// Get a new address
    #[method(name = "get_new_address")]
    async fn get_new_address(&self) -> RpcResult<Address>;

    /// Get the current block count
    #[method(name = "getblockcount")]
    async fn getblockcount(&self) -> RpcResult<u32>;

    /// Attempt to mine a sidechain block
    #[method(name = "mine")]
    async fn mine(&self, fee: Option<u64>) -> RpcResult<()>;

    /// Remove a tx from the mempool
    #[method(name = "remove_from_mempool")]
    async fn remove_from_mempool(&self, txid: Txid) -> RpcResult<()>;

    /// Set the wallet seed from a mnemonic seed phrase
    #[method(name = "set_seed_from_mnemonic")]
    async fn set_seed_from_mnemonic(&self, mnemonic: String) -> RpcResult<()>;

    /// Get total sidechain wealth
    #[method(name = "sidechain_wealth")]
    async fn sidechain_wealth(&self) -> RpcResult<bitcoin::Amount>;

    /// Stop the node
    #[method(name = "stop")]
    async fn stop(&self);

    /// Transfer funds to the specified address
    #[method(name = "transfer")]
    async fn transfer(
        &self,
        dest: Address,
        value_sats: u64,
        fee_sats: u64,
    ) -> RpcResult<Txid>;

    /// Initiate a withdrawal to the specified mainchain address
    #[method(name = "withdraw")]
    async fn withdraw(
        &self,
        mainchain_address: bitcoin::Address<bitcoin::address::NetworkUnchecked>,
        amount_sats: u64,
        fee_sats: u64,
        mainchain_fee_sats: u64,
    ) -> RpcResult<Txid>;
}
