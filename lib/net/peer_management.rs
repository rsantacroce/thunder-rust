use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use borsh::{BorshDeserialize, BorshSerialize};

/// Services offered by a peer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct PeerServices {
    pub services: u64,
}

impl PeerServices {
    pub const NONE: Self = Self { services: 0 };
    pub const NETWORK: Self = Self { services: 1 << 0 };
    pub const BLOOM: Self = Self { services: 1 << 1 };
    pub const WITNESS: Self = Self { services: 1 << 2 };
    pub const COMPACT_FILTERS: Self = Self { services: 1 << 3 };
    pub const NETWORK_LIMITED: Self = Self { services: 1 << 10 };

    pub fn has_service(&self, service: Self) -> bool {
        (self.services & service.services) != 0
    }

    pub fn add_service(&mut self, service: Self) {
        self.services |= service.services;
    }
}

/// Information about a known peer
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize, Copy, PartialEq, Eq, Hash)]
pub struct PeerInfo {
    /// The peer's network address
    pub addr: SocketAddr,
    /// Last time we successfully connected to this peer
    pub last_seen: u64,
    /// Services offered by this peer
    pub services: PeerServices,
    /// The peer that told us about this peer
    pub source: SocketAddr,
    /// Last time we attempted to connect
    pub last_try: u64,
    /// Number of failed connection attempts
    pub attempts: u32,
    /// Whether this peer is in the tried set
    pub is_tried: bool,
}

impl PeerInfo {
    pub fn new(addr: SocketAddr, source: SocketAddr) -> Self {
        Self {
            addr,
            last_seen: 0,
            services: PeerServices::NONE,
            source,
            last_try: 0,
            attempts: 0,
            is_tried: false,
        }
    }

    pub fn update_last_seen(&mut self) {
        self.last_seen = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();
    }

    pub fn update_last_try(&mut self) {
        self.last_try = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();
        self.attempts += 1;
    }
}

/// Errors that can occur during peer management
#[derive(Debug, Error)]
pub enum PeerManagementError {
    #[error("Peer not found: {0}")]
    PeerNotFound(SocketAddr),
    #[error("Database error: {0}")]
    Database(#[from] crate::net::error::Error),
}

/// Manages known peers and their selection
pub struct AddrMan {
    /// Peers we have successfully connected to
    tried: HashMap<u64, Vec<PeerInfo>>,
    /// Peers we know about but haven't connected to
    new: HashMap<u64, Vec<PeerInfo>>,
    /// Map of source peers to the peers they told us about
    source_peers: HashMap<SocketAddr, HashSet<SocketAddr>>,
    /// Maximum number of peers to keep in the tried set
    max_tried_peers: usize,
    /// Maximum number of peers to keep in the new set
    max_new_peers: usize,
}

impl AddrMan {
    pub fn new(max_tried_peers: usize, max_new_peers: usize) -> Self {
        Self {
            tried: HashMap::new(),
            new: HashMap::new(),
            source_peers: HashMap::new(),
            max_tried_peers,
            max_new_peers,
        }
    }

    /// Add a new peer to the address manager
    pub fn add_peer(&mut self, peer: PeerInfo) {
        let bucket = self.get_bucket(&peer.addr);
        
        if peer.is_tried {
            self.tried.entry(bucket).or_default().push(peer);
            if self.tried.values().map(|v| v.len()).sum::<usize>() > self.max_tried_peers {
                self.remove_oldest_tried();
            }
        } else {
            self.new.entry(bucket).or_default().push(peer);
            if self.new.values().map(|v| v.len()).sum::<usize>() > self.max_new_peers {
                self.remove_oldest_new();
            }
        }

        // Update source tracking
        self.source_peers
            .entry(peer.source)
            .or_default()
            .insert(peer.addr.clone());
    }

    /// Get a peer to connect to
    pub fn select_peer(&self) -> Option<SocketAddr> {
        // First try to find a peer from the tried set
        if let Some(peer) = self.select_tried_peer() {
            return Some(peer);
        }

        // If no tried peers are available, try the new set
        self.select_new_peer()
    }

    /// Select a peer from the tried set
    fn select_tried_peer(&self) -> Option<SocketAddr> {
        // TODO: Implement proper selection logic with randomization
        self.tried
            .values()
            .flat_map(|peers| peers.iter())
            .min_by_key(|peer| peer.last_try)
            .map(|peer| peer.addr)
    }

    /// Select a peer from the new set
    fn select_new_peer(&self) -> Option<SocketAddr> {
        // TODO: Implement proper selection logic with randomization
        self.new
            .values()
            .flat_map(|peers| peers.iter())
            .min_by_key(|peer| peer.last_try)
            .map(|peer| peer.addr)
    }

    /// Get the bucket index for an address
    fn get_bucket(&self, addr: &SocketAddr) -> u64 {
        // TODO: Implement proper bucket selection with salting
        // For now, use a simple hash of the address
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        addr.hash(&mut hasher);
        hasher.finish()
    }

    /// Remove the oldest peer from the tried set
    fn remove_oldest_tried(&mut self) {
        // Find the bucket with the oldest peer
        let oldest_bucket = self.tried.iter()
            .min_by_key(|(_, peers)| peers.iter().map(|p| p.last_seen).min().unwrap_or(0))
            .map(|(bucket, _)| *bucket);

        if let Some(bucket) = oldest_bucket {
            if let Some(peers) = self.tried.get_mut(&bucket) {
                if let Some(peer) = peers.iter().min_by_key(|p| p.last_seen) {
                    let addr = peer.addr;
                    peers.retain(|p| p.addr != addr);
                    if peers.is_empty() {
                        self.tried.remove(&bucket);
                    }
                }
            }
        }
    }

    /// Remove the oldest peer from the new set
    fn remove_oldest_new(&mut self) {
        // Find the bucket with the oldest peer
        let oldest_bucket = self.new.iter()
            .min_by_key(|(_, peers)| peers.iter().map(|p| p.last_seen).min().unwrap_or(0))
            .map(|(bucket, _)| *bucket);

        if let Some(bucket) = oldest_bucket {
            if let Some(peers) = self.new.get_mut(&bucket) {
                if let Some(peer) = peers.iter().min_by_key(|p| p.last_seen) {
                    let addr = peer.addr;
                    peers.retain(|p| p.addr != addr);
                    if peers.is_empty() {
                        self.new.remove(&bucket);
                    }
                }
            }
        }
    }

    /// Get all known peers
    pub fn get_all_peers(&self) -> Vec<&PeerInfo> {
        let mut peers = Vec::new();
        peers.extend(self.tried.values().flat_map(|v| v.iter()));
        peers.extend(self.new.values().flat_map(|v| v.iter()));
        peers
    }

    /// Get the number of peers in the tried set
    pub fn tried_peer_count(&self) -> usize {
        self.tried.values().map(|v| v.len()).sum()
    }

    /// Get the number of peers in the new set
    pub fn new_peer_count(&self) -> usize {
        self.new.values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::thread;
    use std::time::Duration;

    fn create_test_addr(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
    }

    #[test]
    fn test_add_peer() {
        let mut addrman = AddrMan::new(100, 100);
        let addr = create_test_addr(8333);
        let source = create_test_addr(8334);
        
        let peer = PeerInfo::new(addr, source);
        addrman.add_peer(peer);
        
        assert_eq!(addrman.new_peer_count(), 1);
        assert_eq!(addrman.tried_peer_count(), 0);
    }

    #[test]
    fn test_peer_services() {
        let mut services = PeerServices::NONE;
        assert!(!services.has_service(PeerServices::NETWORK));
        
        services.add_service(PeerServices::NETWORK);
        assert!(services.has_service(PeerServices::NETWORK));
        
        services.add_service(PeerServices::BLOOM);
        assert!(services.has_service(PeerServices::NETWORK));
        assert!(services.has_service(PeerServices::BLOOM));
    }

    #[test]
    fn test_peer_info_updates() {
        let addr = create_test_addr(8333);
        let source = create_test_addr(8334);
        let mut peer = PeerInfo::new(addr, source);
        
        assert_eq!(peer.attempts, 0);
        assert_eq!(peer.last_seen, 0);
        assert_eq!(peer.last_try, 0);
        
        peer.update_last_try();
        assert_eq!(peer.attempts, 1);
        assert!(peer.last_try > 0);
        
        peer.update_last_seen();
        assert!(peer.last_seen > 0);
    }

    #[test]
    fn test_peer_selection() {
        let mut addrman = AddrMan::new(100, 100);
        let addr1 = create_test_addr(8333);
        let addr2 = create_test_addr(8334);
        let source = create_test_addr(8335);
        
        // Add a tried peer
        let mut peer1 = PeerInfo::new(addr1, source);
        peer1.is_tried = true;
        addrman.add_peer(peer1);
        
        // Add a new peer
        let peer2 = PeerInfo::new(addr2, source);
        addrman.add_peer(peer2);
        
        // Should select tried peer first
        let selected = addrman.select_peer();
        assert!(selected.is_some());
        assert_eq!(selected.unwrap(), addr1);
    }

    #[test]
    fn test_max_peers_limit() {
        let mut addrman = AddrMan::new(2, 2); // Small limits for testing
        
        // Add more peers than the limit
        for i in 0..4 {
            let addr = create_test_addr(8333 + i);
            let source = create_test_addr(8334);
            let mut peer = PeerInfo::new(addr, source);
            peer.is_tried = i < 2; // First two are tried
            addrman.add_peer(peer);
        }
        
        assert_eq!(addrman.tried_peer_count(), 2);
        assert_eq!(addrman.new_peer_count(), 2);
    }

    #[test]
    fn test_oldest_peer_removal() {
        let mut addrman = AddrMan::new(2, 2);
        let source = create_test_addr(8334);
        
        // Add three tried peers
        for i in 0..3 {
            let addr = create_test_addr(8333 + i);
            let mut peer = PeerInfo::new(addr, source);
            peer.is_tried = true;
            thread::sleep(Duration::from_millis(10)); // Ensure different timestamps
            peer.update_last_seen();
            addrman.add_peer(peer);
        }
        
        assert_eq!(addrman.tried_peer_count(), 2); // Should remove oldest
    }

    #[test]
    fn test_get_all_peers() {
        let mut addrman = AddrMan::new(100, 100);
        let source = create_test_addr(8334);
        
        // Add some peers
        for i in 0..5 {
            let addr = create_test_addr(8333 + i);
            let mut peer = PeerInfo::new(addr, source);
            peer.is_tried = i < 3; // First three are tried
            addrman.add_peer(peer);
        }
        
        let all_peers = addrman.get_all_peers();
        assert_eq!(all_peers.len(), 5);
        assert_eq!(addrman.tried_peer_count(), 3);
        assert_eq!(addrman.new_peer_count(), 2);
    }

    #[test]
    fn test_peer_bucketing() {
        let mut addrman = AddrMan::new(100, 100);
        let source = create_test_addr(8334);
        
        // Add peers with different IPs
        let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 8333);
        let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 8333);
        
        let peer1 = PeerInfo::new(addr1, source);
        let peer2 = PeerInfo::new(addr2, source);
        
        addrman.add_peer(peer1);
        addrman.add_peer(peer2);
        
        assert_eq!(addrman.new_peer_count(), 2);
    }

    #[test]
    fn test_addr_message_creation() {
        let addr1 = create_test_addr(8333);
        let addr2 = create_test_addr(8334);
        let source = create_test_addr(8335);
        
        let mut peer1 = PeerInfo::new(addr1, source);
        let mut peer2 = PeerInfo::new(addr2, source);
        peer1.services.add_service(PeerServices::NETWORK);
        peer2.services.add_service(PeerServices::BLOOM);
        
        let peers = vec![peer1, peer2];
        let addr_message = AddrMessage::from(peers.clone());
        
        assert_eq!(addr_message.addresses.len(), 2);
        assert_eq!(addr_message.addresses[0].addr, addr1);
        assert_eq!(addr_message.addresses[1].addr, addr2);
        assert!(addr_message.addresses[0].services.has_service(PeerServices::NETWORK));
        assert!(addr_message.addresses[1].services.has_service(PeerServices::BLOOM));
    }

    #[test]
    fn test_addr_message_handling() {
        let mut addrman = AddrMan::new(100, 100);
        let addr1 = create_test_addr(8333);
        let addr2 = create_test_addr(8334);
        let source = create_test_addr(8335);
        
        // Create and add initial peer
        let mut peer1 = PeerInfo::new(addr1, source);
        peer1.services.add_service(PeerServices::NETWORK);
        addrman.add_peer(peer1);
        
        // Create addr message with new peer
        let mut peer2 = PeerInfo::new(addr2, source);
        peer2.services.add_service(PeerServices::BLOOM);
        let addr_message = AddrMessage::from(vec![peer2]);
        
        // Simulate receiving addr message
        for peer_info in addr_message.addresses {
            addrman.add_peer(peer_info);
        }
        
        // Verify both peers are now in addrman
        let all_peers = addrman.get_all_peers();
        assert_eq!(all_peers.len(), 2);
        assert!(all_peers.iter().any(|p| p.addr == addr1));
        assert!(all_peers.iter().any(|p| p.addr == addr2));
    }

    #[test]
    fn test_addr_message_duplicate_handling() {
        let mut addrman = AddrMan::new(100, 100);
        let addr = create_test_addr(8333);
        let source = create_test_addr(8334);
        
        // Create and add initial peer
        let mut peer = PeerInfo::new(addr, source);
        peer.services.add_service(PeerServices::NETWORK);
        addrman.add_peer(peer);
        
        // Create addr message with the same peer
        let mut peer_duplicate = PeerInfo::new(addr, source);
        peer_duplicate.services.add_service(PeerServices::BLOOM);
        let addr_message = AddrMessage::from(vec![peer_duplicate]);
        
        // Simulate receiving addr message
        for peer_info in addr_message.addresses {
            addrman.add_peer(peer_info);
        }
        
        // Verify only one instance of the peer exists
        let all_peers = addrman.get_all_peers();
        assert_eq!(all_peers.len(), 1);
        assert_eq!(all_peers[0].addr, addr);
        // Verify services were updated
        assert!(all_peers[0].services.has_service(PeerServices::BLOOM));
    }

    #[test]
    fn test_addr_message_broadcast() {
        let mut addrman = AddrMan::new(100, 100);
        let source = create_test_addr(8334);
        
        // Add multiple peers
        for i in 0..5 {
            let addr = create_test_addr(8333 + i);
            let mut peer = PeerInfo::new(addr, source);
            peer.services.add_service(PeerServices::NETWORK);
            addrman.add_peer(peer);
        }
        
        // Get all peers for broadcast
        let all_peers = addrman.get_all_peers();
        let addr_message = AddrMessage::from(all_peers.iter().map(|p| **p).collect::<Vec<_>>());
        
        // Verify broadcast message contains all peers
        assert_eq!(addr_message.addresses.len(), 5);
        for i in 0..5 {
            let addr = create_test_addr(8333 + i);
            assert!(addr_message.addresses.iter().any(|p| p.addr == addr));
        }
    }

    #[test]
    fn test_addr_message_services_preservation() {
        let mut addrman = AddrMan::new(100, 100);
        let addr = create_test_addr(8333);
        let source = create_test_addr(8334);
        
        // Create peer with multiple services
        let mut peer = PeerInfo::new(addr, source);
        peer.services.add_service(PeerServices::NETWORK);
        peer.services.add_service(PeerServices::BLOOM);
        peer.services.add_service(PeerServices::WITNESS);
        
        // Add to addrman
        addrman.add_peer(peer);
        
        // Create addr message
        let all_peers = addrman.get_all_peers();
        let addr_message = AddrMessage::from(all_peers.iter().map(|p| **p).collect::<Vec<_>>());
        
        // Verify services are preserved in the message
        assert_eq!(addr_message.addresses.len(), 1);
        let peer_services = addr_message.addresses[0].services;
        assert!(peer_services.has_service(PeerServices::NETWORK));
        assert!(peer_services.has_service(PeerServices::BLOOM));
        assert!(peer_services.has_service(PeerServices::WITNESS));
    }
} 