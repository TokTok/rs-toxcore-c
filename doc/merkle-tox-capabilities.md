# Merkle-Tox Sub-Design: Capabilities & Handshake

## Overview

Peers MUST negotiate capabilities before initiating a synchronization session to
prevent protocol mismatches.

## 1. Capability Discovery

The handshake uses `tox-sequenced`.

### Discovery via Custom Packet

1.  Upon connection, a client sends a `CAPS_ANNOUNCE` message via a
    `tox-sequenced` DATA packet over custom lossy packets.
2.  If the peer supports Merkle-Tox, they respond with their own
    `CAPS_ANNOUNCE`.

## 2. Capability Negotiation

### A. Network-Intrinsic (Ephemeral)

Negotiated per-session via the `CAPS_ANNOUNCE` packet.

Serialized via MessagePack:

```rust
struct CapsAnnounce {
    /// Protocol version (e.g., 1).
    version: u32,
    /// Bitmask of optional transport features:
    /// 0x01: Multi-Source Swarm Sync (merkle-tox-cas.md)
    /// 0x02: Advanced Set Reconciliation (IBLT / tox-reconcile)
    /// 0x04: Large Batch Support (> 100 nodes per FETCH_BATCH_REQ)
    features: u64,
}
```

### B. Data-Intrinsic (Persistent / Baseline)

Mandatory for Version 1. Committed to the DAG (in **Genesis Node** or
**Announcement Nodes**). Every member MUST support these to parse history.

*   **Signed ECIES Handshake**: Mandatory for initial $K_{conv}$ establishment.
*   **Symmetric Ratcheting**: Mandatory for post-compromise security and hash
    chaining.
*   **Power-of-2 Padding**: Mandatory ISO/IEC 7816-4 anti-traffic analysis.
*   **Compression**: (e.g., Zstd) If used by an author, it must be supported by
    all readers.

**Principle**: Data-Intrinsic features are immutable history properties. A peer
MUST NOT author a node using an optional Data-Intrinsic feature unless enabled
in the room's Genesis parameters.

### `CAPS_ACK` (Packet Content)

Peer B responds with `CapsAnnounce` via a `CAPS_ACK` message (0x02), completing
the handshake. The `merkle-tox-sync` state machine begins.

## 3. Post-Handshake Procedures

After `CAPS_ACK`:

1.  **Head Exchange**: Both peers send `SYNC_HEADS` to begin history
    reconciliation.
2.  **Continuous Clock Sync**: Peers monitor transport PING/PONG timestamps to
    maintain network time offsets.
