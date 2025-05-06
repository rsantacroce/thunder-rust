//! P2P connection tests

use std::net::SocketAddr;

use bip300301_enforcer_integration_tests::{
    integration_test::{activate_sidechain, fund_enforcer, propose_sidechain},
    setup::{
        Mode, Network, PostSetup as EnforcerPostSetup, Sidechain as _,
        setup as setup_enforcer,
    },
    util::{AbortOnDrop, AsyncTrial},
};
use futures::{FutureExt, StreamExt as _, channel::mpsc, future::BoxFuture};
use thunder_app_rpc_api::RpcClient as _;
use tokio::time::sleep;
use tracing::Instrument as _;

use crate::{
    setup::{Init, PostSetup},
    util::BinPaths,
};

#[derive(Debug)]
struct ThunderNodes {
    /// Sidechain process that will be sending blocks
    sender: PostSetup,
    /// Second sidechain process that will be sending blocks
    sender_2: PostSetup,
    /// The sidechain instance that will be syncing blocks
    syncer: PostSetup,
    /// Second sidechain instance that will be syncing blocks
    syncer_2: PostSetup,
}


/// Initial setup for the test
async fn setup(
    bin_paths: BinPaths,
    res_tx: mpsc::UnboundedSender<anyhow::Result<()>>,
) -> anyhow::Result<(EnforcerPostSetup, ThunderNodes)> {
    tracing::info!("Setting up enforcer");
    let mut enforcer_post_setup = setup_enforcer(
        &bin_paths.others,
        Network::Regtest,
        Mode::Mempool,
        res_tx.clone(),
    )
    .await?;
    tracing::info!("Setting up thunder send node");
    let sidechain_sender = PostSetup::setup(
        Init {
            thunder_app: bin_paths.thunder.clone(),
            data_dir_suffix: Some("sender".to_owned()),
        },
        &enforcer_post_setup,
        res_tx.clone(),
    )
    .await?;
    tracing::info!("Setup thunder send node successfully");

    tracing::info!("Setting up thunder send node 2");
    let sidechain_sender_2 = PostSetup::setup(
        Init {
            thunder_app: bin_paths.thunder.clone(),
            data_dir_suffix: Some("sender_2".to_owned()),
        },
        &enforcer_post_setup,
        res_tx.clone(),
    )
    .await?;
    tracing::info!("Setup thunder send node 2 successfully");

    let sidechain_syncer = PostSetup::setup(
        Init {
            thunder_app: bin_paths.thunder.clone(),
            data_dir_suffix: Some("syncer".to_owned()),
        },
        &enforcer_post_setup,
        res_tx.clone(),
    )
    .await?;
    tracing::info!("Setup thunder sync node successfully");

    let sidechain_syncer_2 = PostSetup::setup(
        Init {
            thunder_app: bin_paths.thunder.clone(),
            data_dir_suffix: Some("syncer_2".to_owned()),
        },
        &enforcer_post_setup,
        res_tx.clone(),
    )
    .await?;
    tracing::info!("Setup thunder sync node 2 successfully");

    let thunder_nodes = ThunderNodes {
        sender: sidechain_sender,
        sender_2: sidechain_sender_2,
        syncer: sidechain_syncer,
        syncer_2: sidechain_syncer_2,
    };
    tracing::info!("Setup successfully");
    let () = propose_sidechain::<PostSetup>(&mut enforcer_post_setup).await?;
    tracing::info!("Proposed sidechain successfully");
    let () = activate_sidechain::<PostSetup>(&mut enforcer_post_setup).await?;
    tracing::info!("Activated sidechain successfully");
    let () = fund_enforcer::<PostSetup>(&mut enforcer_post_setup).await?;
    Ok((enforcer_post_setup, thunder_nodes))
}

/// Check that a Thunder node is connected to the specified peer
async fn check_peer_connection(
    thunder_setup: &PostSetup,
    expected_peer: SocketAddr,
) -> anyhow::Result<()> {
    let peers = thunder_setup
        .rpc_client
        .list_peers()
        .await?
        .iter()
        .map(|p| p.address)
        .collect::<Vec<_>>();

    if peers.contains(&expected_peer) {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Expected connection to {expected_peer}, found {peers:?}"
        ))
    }
}

async fn p2p_task(
    bin_paths: BinPaths,
    res_tx: mpsc::UnboundedSender<anyhow::Result<()>>,
) -> anyhow::Result<()> {
    let (mut enforcer_post_setup, thunder_nodes) =
        setup(bin_paths, res_tx).await?;
    const BMM_BLOCKS: u32 = 16;
    tracing::info!(blocks = %BMM_BLOCKS, "Attempting BMM");
    thunder_nodes
        .sender
        .bmm(&mut enforcer_post_setup, BMM_BLOCKS)
        .await?;
    // Check that sender has all blocks, and syncers have 0
    {
        let sender_blocks =
            thunder_nodes.sender.rpc_client.getblockcount().await?;
        anyhow::ensure!(sender_blocks == BMM_BLOCKS);
        let sender_2_blocks =
            thunder_nodes.sender_2.rpc_client.getblockcount().await?;
        anyhow::ensure!(sender_2_blocks == 0);
        let syncer_blocks =
            thunder_nodes.syncer.rpc_client.getblockcount().await?;
        anyhow::ensure!(syncer_blocks == 0);
        let syncer_2_blocks =
            thunder_nodes.syncer_2.rpc_client.getblockcount().await?;
        anyhow::ensure!(syncer_2_blocks == 0);
    }

    // Print initial peer information for all nodes
    tracing::info!("Initial peer connections:");
    let sender_peers = thunder_nodes.sender.rpc_client.list_peers().await?;
    let sender_2_peers = thunder_nodes.sender_2.rpc_client.list_peers().await?;
    let syncer_peers = thunder_nodes.syncer.rpc_client.list_peers().await?;
    let syncer_2_peers = thunder_nodes.syncer_2.rpc_client.list_peers().await?;
    tracing::info!("Sender 1 peers: {:?}", sender_peers.iter().map(|p| p.address).collect::<Vec<_>>());
    tracing::info!("Sender 2 peers: {:?}", sender_2_peers.iter().map(|p| p.address).collect::<Vec<_>>());
    tracing::info!("Syncer 1 peers: {:?}", syncer_peers.iter().map(|p| p.address).collect::<Vec<_>>());
    tracing::info!("Syncer 2 peers: {:?}", syncer_2_peers.iter().map(|p| p.address).collect::<Vec<_>>());

    tracing::info!("Attempting sync");
    tracing::debug!(
        sender_addr = %thunder_nodes.sender.net_addr(),
        sender_2_addr = %thunder_nodes.sender_2.net_addr(),
        syncer_addr = %thunder_nodes.syncer.net_addr(),
        syncer_2_addr = %thunder_nodes.syncer_2.net_addr(),
        "Connecting syncers to senders");
    
    // Connect both syncers to both senders
    let () = thunder_nodes
        .syncer
        .rpc_client
        .connect_peer(thunder_nodes.sender.net_addr().into())
        .await?;
    let () = thunder_nodes
        .syncer
        .rpc_client
        .connect_peer(thunder_nodes.sender_2.net_addr().into())
        .await?;
    let () = thunder_nodes
        .syncer_2
        .rpc_client
        .connect_peer(thunder_nodes.sender.net_addr().into())
        .await?;
    let () = thunder_nodes
        .syncer_2
        .rpc_client
        .connect_peer(thunder_nodes.sender_2.net_addr().into())
        .await?;

    // Wait for connection to be established
    sleep(std::time::Duration::from_secs(1)).await;
    tracing::debug!("Checking peer connections");
    
    // Check peer connections for both syncers
    let () = check_peer_connection(
        &thunder_nodes.syncer,
        thunder_nodes.sender.net_addr().into(),
    )
    .await?;
    let () = check_peer_connection(
        &thunder_nodes.syncer,
        thunder_nodes.sender_2.net_addr().into(),
    )
    .await?;
    let () = check_peer_connection(
        &thunder_nodes.syncer_2,
        thunder_nodes.sender.net_addr().into(),
    )
    .await?;
    let () = check_peer_connection(
        &thunder_nodes.syncer_2,
        thunder_nodes.sender_2.net_addr().into(),
    )
    .await?;
    tracing::debug!("Both syncers have connections to both senders");

    let () = check_peer_connection(
        &thunder_nodes.sender,
        thunder_nodes.syncer.net_addr().into(),
    )
    .await?;
    let () = check_peer_connection(
        &thunder_nodes.sender,
        thunder_nodes.syncer_2.net_addr().into(),
    )
    .await?;
    tracing::debug!("Sender 1 has connections to both syncers");

    let () = check_peer_connection(
        &thunder_nodes.sender_2,
        thunder_nodes.syncer.net_addr().into(),
    )
    .await?;
    let () = check_peer_connection(
        &thunder_nodes.sender_2,
        thunder_nodes.syncer_2.net_addr().into(),
    )
    .await?;
    tracing::debug!("Sender 2 has connections to both syncers");

    // Wait for sync to occur
    sleep(std::time::Duration::from_secs(10)).await;

    // Print final peer information for all nodes
    tracing::info!("Final peer connections after sync:");
    let sender_peers = thunder_nodes.sender.rpc_client.list_peers().await?;
    let sender_2_peers = thunder_nodes.sender_2.rpc_client.list_peers().await?;
    let syncer_peers = thunder_nodes.syncer.rpc_client.list_peers().await?;
    let syncer_2_peers = thunder_nodes.syncer_2.rpc_client.list_peers().await?;
    tracing::info!("Sender 1 peers: {:?}", sender_peers.iter().map(|p| p.address).collect::<Vec<_>>());
    tracing::info!("Sender 2 peers: {:?}", sender_2_peers.iter().map(|p| p.address).collect::<Vec<_>>());
    tracing::info!("Syncer 1 peers: {:?}", syncer_peers.iter().map(|p| p.address).collect::<Vec<_>>());
    tracing::info!("Syncer 2 peers: {:?}", syncer_2_peers.iter().map(|p| p.address).collect::<Vec<_>>());

    // Check that all nodes have the correct number of blocks
    {
        let sender_blocks =
            thunder_nodes.sender.rpc_client.getblockcount().await?;
        anyhow::ensure!(sender_blocks == BMM_BLOCKS);
        let syncer_blocks =
            thunder_nodes.syncer.rpc_client.getblockcount().await?;
        anyhow::ensure!(syncer_blocks == BMM_BLOCKS);
        let syncer_2_blocks =
            thunder_nodes.syncer_2.rpc_client.getblockcount().await?;
        anyhow::ensure!(syncer_2_blocks == BMM_BLOCKS);
    }
    drop(thunder_nodes.syncer);
    drop(thunder_nodes.syncer_2);
    drop(thunder_nodes.sender);
    drop(thunder_nodes.sender_2);
    tracing::info!("Removing {}", enforcer_post_setup.out_dir.path().display());
    drop(enforcer_post_setup.tasks);
    // Wait for tasks to die
    sleep(std::time::Duration::from_secs(1)).await;
    enforcer_post_setup.out_dir.cleanup()?;
    Ok(())
}

async fn p2p(bin_paths: BinPaths) -> anyhow::Result<()> {
    let (res_tx, mut res_rx) = mpsc::unbounded();
    let _test_task: AbortOnDrop<()> = tokio::task::spawn({
        let res_tx = res_tx.clone();
        async move {
            let res =
            p2p_task(bin_paths, res_tx.clone()).await;
            let _send_err: Result<(), _> = res_tx.unbounded_send(res);
        }
        .in_current_span()
    })
    .into();
    res_rx.next().await.ok_or_else(|| {
        anyhow::anyhow!("Unexpected end of test task result stream")
    })?
}

pub fn p2p_trial(
    bin_paths: BinPaths,
) -> AsyncTrial<BoxFuture<'static, anyhow::Result<()>>> {
    AsyncTrial::new("p2p_task", p2p(bin_paths).boxed())
}

