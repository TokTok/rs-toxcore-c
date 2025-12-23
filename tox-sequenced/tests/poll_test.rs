use rand::SeedableRng;
use std::time::{Duration, Instant};
use tox_proto::TimeProvider;
use tox_sequenced::protocol::{FragmentCount, FragmentIndex, MessageId, Packet, TimestampMs};
use tox_sequenced::rtt::INITIAL_RTO;
use tox_sequenced::session::PING_INTERVAL_IDLE;
use tox_sequenced::{MessageType, SequenceSession};

#[test]
fn test_next_wakeup_pacing() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp, &mut rng);

    // Perform initial poll to clear the overdue startup ping
    let _ = session.get_packets_to_send(now, 0);

    // Default wakeup should be long (keepalive/idle)
    let wakeup = session.next_wakeup(now);
    assert!(wakeup > now + Duration::from_secs(4));

    // Author a message. Since window is open, it should want immediate poll
    let _ = session.send_message(MessageType::SyncHeads, b"pacing test", now);
    let wakeup = session.next_wakeup(now);
    assert_eq!(wakeup, now);
}

#[test]
fn test_next_wakeup_ack_delay() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp, &mut rng);

    // Create a valid reassembled payload (MessageType + Payload)
    use tox_proto::ToxProto;
    #[derive(ToxProto)]
    struct Envelope {
        mtype: MessageType,
        payload: Vec<u8>,
    }
    let env = Envelope {
        mtype: MessageType::SyncHeads,
        payload: b"hello".to_vec(),
    };
    let data = tox_proto::serialize(&env).unwrap();

    // 3. Receive a single-fragment message for Bob.
    let packet = Packet::Data {
        message_id: MessageId(1),
        fragment_index: FragmentIndex(0),
        total_fragments: FragmentCount(1),
        data,
    };

    session.handle_packet(packet, now);

    // Verify message was received correctly
    let mut found = false;
    while let Some(event) = session.poll_event() {
        if let tox_sequenced::SessionEvent::MessageCompleted(id, mtype, payload) = event {
            assert_eq!(id, MessageId(1));
            assert_eq!(mtype, MessageType::SyncHeads);
            assert_eq!(payload, b"hello");
            found = true;
        }
    }
    assert!(found);

    // Next wakeup should be immediate because count >= 2 for completed message
    let wakeup = session.next_wakeup(now);
    assert_eq!(wakeup, now);
}

#[test]
fn test_next_wakeup_rto() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp, &mut rng);

    // Send a message
    let _ = session.send_message(MessageType::SyncHeads, b"rto test", now);

    // Initial packet
    let packets = session.get_packets_to_send(now, 0);
    assert!(!packets.is_empty());

    // Advance past pacing delay
    let now_after_pacing = now + Duration::from_millis(100);

    // Next wakeup should be either Tail Loss Probe or Retransmission Timeout.
    // Both are relatively short compared to idle keepalive.
    let wakeup = session.next_wakeup(now_after_pacing);
    assert!(wakeup > now);
    assert!(wakeup <= now + Duration::from_secs(2));
}

#[test]
fn test_next_wakeup_never_past() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp.clone(), &mut rng);

    // 1. Force a delayed ACK
    use tox_proto::ToxProto;
    #[derive(ToxProto)]
    struct Envelope {
        mtype: MessageType,
        payload: Vec<u8>,
    }
    let env = Envelope {
        mtype: MessageType::SyncHeads,
        payload: b"hello".to_vec(),
    };
    let data = tox_proto::serialize(&env).unwrap();

    let packet = Packet::Data {
        message_id: MessageId(1),
        fragment_index: FragmentIndex(0),
        total_fragments: FragmentCount(1),
        data,
    };

    session.handle_packet(packet, now);

    // Advance time PAST the delayed ACK timeout
    tp.advance(Duration::from_millis(100));
    let future_now = tp.now_instant();

    // next_wakeup should not be in the past
    let wakeup = session.next_wakeup(future_now);
    assert!(
        wakeup >= future_now,
        "Wakeup {:?} should not be in the past of {:?}",
        wakeup,
        future_now
    );
}

#[test]
fn test_next_wakeup_idle_alignment() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp.clone(), &mut rng);

    // Initial state: idle, last_ping set to long ago in constructor (CONNECTION_TIMEOUT).
    // Perform one poll to normalize state.
    session.get_packets_to_send(now, 0);

    // Advance 10 seconds. Session is idle.
    // Ping is due at now + PING_INTERVAL_IDLE (60s).
    let now_10s = now + Duration::from_secs(10);
    assert!(Duration::from_secs(10) < PING_INTERVAL_IDLE);
    tp.set_time(now_10s, 10000);

    let wakeup = session.next_wakeup(now_10s);

    // If it's returning 'now' or something very soon, and get_packets_to_send does nothing, it's a loop.
    let packets = session.get_packets_to_send(now_10s, 10000);
    if packets.is_empty() {
        // If no packets are sent, wakeup MUST be in the future.
        assert!(
            wakeup > now_10s,
            "Tight loop detected! Wakeup is {:?} but no work to do.",
            wakeup
        );
    }
}

#[test]
fn test_next_wakeup_zero_window_no_loop() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp.clone(), &mut rng);

    // Initial poll to clear startup ping
    let _ = session.get_packets_to_send(now, 0);

    // 1. Force peer rwnd to 0 by receiving an ACK with rwnd=0
    let ack = tox_sequenced::protocol::SelectiveAck {
        message_id: MessageId(0),
        base_index: FragmentIndex(0),
        bitmask: 0,
        rwnd: FragmentCount(0), // ZERO WINDOW
    };
    session.handle_packet(Packet::Ack(ack), now);

    // 2. Queue a message to send
    let _ = session.send_message(MessageType::SyncHeads, b"test", now);

    // 3. next_wakeup SHOULD NOT be 'now' because we can't send anything due to zero window.
    // (Except probes, but they have their own timer)
    let wakeup = session.next_wakeup(now);

    let packets = session.get_packets_to_send(now, 0);
    if packets.is_empty() {
        assert!(
            wakeup > now,
            "Tight loop in zero window state! Wakeup is 'now' but no packets can be sent."
        );
    }
}

#[test]
fn test_next_wakeup_rto_backoff_loop() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp.clone(), &mut rng);

    // Initial poll
    let _ = session.get_packets_to_send(now, 0);

    // 1. Send a message
    let _ = session
        .send_message(MessageType::SyncHeads, b"test", now)
        .unwrap();
    let _ = session.get_packets_to_send(now, 0);
    // last_sent = 0. Queue = [(0, 0)].

    // 2. Trigger TLP (Tail Loss Probe). srtt=200ms, TLP=300ms.
    tp.advance(Duration::from_millis(350));
    let now_tlp = tp.now_instant();
    let packets = session.get_packets_to_send(now_tlp, 350);
    assert_eq!(packets.len(), 1, "TLP should have triggered");
    // last_sent = 350. Queue = [(0, 0), (0, 350)].
    // TLP re-sends WITHOUT popping.

    // 3. Trigger standard RTO retransmission (INITIAL_RTO from TLP send time).
    tp.set_time(
        now_tlp + INITIAL_RTO + Duration::from_millis(10),
        (now_tlp + INITIAL_RTO + Duration::from_millis(10))
            .saturating_duration_since(now)
            .as_millis() as i64,
    );
    let now_rto = tp.now_instant();
    let packets = session.get_packets_to_send(
        now_rto,
        now_rto.saturating_duration_since(now).as_millis() as u64,
    );
    assert_eq!(packets.len(), 1, "RTO should have triggered");

    // last_sent = now_rto. rto_backoff = 1.
    // Next RTO due at now_rto + (INITIAL_RTO * 2^1).

    // 4. Advance time past the obsolete entry's RTO (T=now_tlp + INITIAL_RTO)
    // but before the probe's RTO (now_rto + INITIAL_RTO * 2).
    tp.set_time(
        now_rto + Duration::from_millis(100),
        (now_rto + Duration::from_millis(100))
            .saturating_duration_since(now)
            .as_millis() as i64,
    );
    let now_loop = tp.now_instant();

    let wakeup = session.next_wakeup(now_loop);
    let packets = session.get_packets_to_send(
        now_loop,
        now_loop.saturating_duration_since(now).as_millis() as u64,
    );

    eprintln!(
        "NOW_LOOP: {:?}, WAKEUP: {:?}, DIFF: {:?}",
        now_loop,
        wakeup,
        wakeup.saturating_duration_since(now_loop)
    );
    eprintln!("PACKETS: {}", packets.len());

    if packets.is_empty() {
        assert!(
            wakeup > now_loop,
            "Tight loop during RTO backoff! Wakeup is 'now' but obsolete queue entry prevents sleep."
        );
    }
}

#[test]
fn test_next_wakeup_rto_backoff_no_loop() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp.clone(), &mut rng);

    // 1. Send a message
    let _ = session
        .send_message(MessageType::SyncHeads, b"test", now)
        .unwrap();
    let packets = session.get_packets_to_send(now, 0);
    assert!(!packets.is_empty());

    // 2. Advance time to trigger RTO (initial RTO is 600ms)
    tp.advance(Duration::from_millis(700));
    let now_rto = tp.now_instant();

    // 3. First RTO should trigger
    let packets = session.get_packets_to_send(now_rto, 700);
    assert!(!packets.is_empty(), "RTO should have triggered after 700ms");
    // rto_backoff for the fragment is now 1.

    // 4. Advance time more, enough to trigger next_wakeup's simple RTO but NOT enough for backed-off RTO
    // next_wakeup wants: 700ms + 600ms = 1300ms
    // get_packets_to_send wants: 700ms + 1200ms = 1900ms
    tp.advance(Duration::from_millis(700)); // Total 1400ms
    let now_loop = tp.now_instant();

    // next_wakeup will return 'now' (or a time in the past) because 1300ms < 1400ms
    let wakeup = session.next_wakeup(now_loop);

    let packets = session.get_packets_to_send(now_loop, 1400);
    if packets.is_empty() {
        assert!(
            wakeup > now_loop,
            "Tight loop during RTO backoff! Wakeup is {:?} but no work to do.",
            wakeup
        );
    }
}

#[test]
fn test_next_wakeup_tlp_no_loop() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp.clone(), &mut rng);

    // Initial poll to clear startup ping
    let _ = session.get_packets_to_send(now, 0);

    // 1. Send a message first so it's in flight
    let _ = session
        .send_message(MessageType::SyncHeads, b"test", now)
        .unwrap();
    let packets = session.get_packets_to_send(now, 0);
    assert!(!packets.is_empty(), "Initial packet should be sent");
    // in_flight > 0. last_sent = 0. Queue = [(0, 0)].

    // 2. Force peer rwnd to 0 using a different message ID
    let ack = tox_sequenced::protocol::SelectiveAck {
        message_id: MessageId(999),
        base_index: FragmentIndex(0),
        bitmask: 0,
        rwnd: FragmentCount(0),
    };
    session.handle_packet(Packet::Ack(ack), now);

    // 3. Advance time to trigger TLP (1.5 * SRTT_initial = 300ms).
    tp.advance(Duration::from_millis(400));
    let now_tlp = tp.now_instant();
    let packets = session.get_packets_to_send(now_tlp, 400);
    assert_eq!(packets.len(), 1, "TLP should have triggered");
    // last_sent = 400. Queue = [(0, 0), (0, 400)].

    // 4. Increase SRTT to 2s to delay FUTURE TLPs
    let pong = Packet::Pong {
        t1: TimestampMs(0),
        t2: TimestampMs(2000),
        t3: TimestampMs(2000),
    };
    session.handle_packet(pong, now_tlp);
    // SRTT is now updated. TLP threshold for the NEXT probe will be ~3000ms.
    // Next TLP due at 400 + 3000 = 3400.
    // Next RTO due at 400 + 1000 = 1400.

    // 5. Advance time past the obsolete entry's RTO (T=0 + 1000 = 1000),
    // but before ALL other timers (Next RTO at 1400, Next TLP at 1037).
    tp.set_time(now + Duration::from_millis(1010), 1010);
    let now_loop = tp.now_instant();

    let wakeup = session.next_wakeup(now_loop);
    let packets = session.get_packets_to_send(now_loop, 1010);

    eprintln!(
        "NOW_LOOP: {:?}, WAKEUP: {:?}, DIFF: {:?}",
        now_loop,
        wakeup,
        wakeup.saturating_duration_since(now_loop)
    );
    eprintln!("PACKETS: {}", packets.len());

    if packets.is_empty() {
        assert!(
            wakeup > now_loop,
            "Tight loop after TLP! Wakeup is 'now' but obsolete queue entry prevents sleep."
        );
    }
}

#[test]
fn test_next_wakeup_probe_pacing_no_loop() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp.clone(), &mut rng);

    // Initial poll
    let _ = session.get_packets_to_send(now, 0);

    // 1. Force peer rwnd to 0 using a different message ID
    let ack = tox_sequenced::protocol::SelectiveAck {
        message_id: MessageId(999),
        base_index: FragmentIndex(0),
        bitmask: 0,
        rwnd: FragmentCount(0),
    };
    session.handle_packet(Packet::Ack(ack), now);

    // 2. Send a message to trigger probes
    let _ = session
        .send_message(MessageType::SyncHeads, b"test", now)
        .unwrap();

    // 3. Advance time to trigger probe RTO (1000ms)
    tp.advance(Duration::from_millis(1100));
    let now_probe = tp.now_instant();

    // 4. Send probe. This sets next_pacing_time in the future.
    let packets = session.get_packets_to_send(now_probe, 1100);
    assert_eq!(packets.len(), 1);

    // 5. next_wakeup SHOULD NOT be 'now' because next_pacing_time is in the future.
    let wakeup = session.next_wakeup(now_probe);
    assert!(
        wakeup > now_probe,
        "Tight loop! Wakeup is 'now' but pacing is active."
    );
}

#[test]
fn test_next_wakeup_datagram_pacing() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp.clone(), &mut rng);

    // Initial poll to clear startup ping
    let _ = session.get_packets_to_send(now, 0);

    // 1. Queue a datagram
    session
        .send_datagram(MessageType::MerkleNode, b"unreliable")
        .unwrap();

    // 2. Initial wakeup should be 'now' to send it
    let wakeup = session.next_wakeup(now);
    assert_eq!(wakeup, now);

    // 3. Send it. This sets next_pacing_time.
    let packets = session.get_packets_to_send(now, 0);
    assert_eq!(packets.len(), 1);

    // 4. Queue another datagram immediately
    session
        .send_datagram(MessageType::MerkleNode, b"unreliable 2")
        .unwrap();

    // 5. next_wakeup should be in the future (next_pacing_time)
    let wakeup = session.next_wakeup(now);
    assert!(
        wakeup > now,
        "Should wait for pacing before sending next datagram"
    );
}

#[test]
fn test_next_wakeup_overdue_rto_never_past() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp.clone(), &mut rng);

    // 1. Send a message
    let _ = session
        .send_message(MessageType::SyncHeads, b"test", now)
        .unwrap();
    let _ = session.get_packets_to_send(now, 0);

    // 2. Advance time past the RTO
    tp.advance(INITIAL_RTO * 2);
    let future_now = tp.now_instant();

    // 3. next_wakeup should return future_now, NOT now (which is 2s in the past)
    let wakeup = session.next_wakeup(future_now);
    assert!(
        wakeup >= future_now,
        "Wakeup {:?} should not be in the past of {:?}",
        wakeup,
        future_now
    );
}

#[test]
fn test_next_wakeup_rwnd_edge_case() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp.clone(), &mut rng);

    // 1. Queue a message. Fragment size will be ~1353.
    let _ = session
        .send_message(MessageType::SyncHeads, b"test", now)
        .unwrap();

    // 2. Set peer rwnd to exactly 1300.
    let ack = tox_sequenced::protocol::SelectiveAck {
        message_id: MessageId(999),
        base_index: FragmentIndex(0),
        bitmask: 0,
        rwnd: FragmentCount(1), // 1 * 1300 = 1300 bytes
    };
    session.handle_packet(Packet::Ack(ack), now);

    // 3. next_wakeup SHOULD NOT be 'now' because fragment is 1353 bytes > 1300 window.
    let wakeup = session.next_wakeup(now);

    let packets = session.get_packets_to_send(now, 0);
    if packets.is_empty() {
        assert!(
            wakeup > now,
            "Tight loop! next_wakeup returned 'now' but window is too small for fragment."
        );
    }
}

#[test]
fn test_next_wakeup_cwnd_limited_no_loop() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp.clone(), &mut rng);

    // 1. Queue a large message (many fragments)
    let data = vec![0u8; 100000]; // ~77 fragments
    let _ = session
        .send_message(MessageType::SyncHeads, &data, now)
        .unwrap();

    // 2. Initial poll will send fragments until cwnd is reached.
    let packets = session.get_packets_to_send(now, 0);
    assert!(!packets.is_empty());

    // Now session is cwnd limited.
    // 3. next_wakeup SHOULD NOT be 'now' because we can't send any more fragments.
    let wakeup = session.next_wakeup(now);

    let next_packets = session.get_packets_to_send(now, 0);
    if next_packets.is_empty() {
        assert!(
            wakeup > now,
            "Tight loop! next_wakeup returned 'now' but session is cwnd limited."
        );
    }
}

#[test]
fn test_next_wakeup_retransmit_cwnd_loop() {
    let now = Instant::now();
    let tp = std::sync::Arc::new(tox_sequenced::time::ManualTimeProvider::new(now, 0));
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let mut session = SequenceSession::new_at(now, tp.clone(), &mut rng);

    // Initial poll
    let _ = session.get_packets_to_send(now, 0);

    // 1. Queue a message and send it
    let data = vec![0u8; 5000]; // 4 fragments
    let msg_id = session
        .send_message(MessageType::SyncHeads, &data, now)
        .unwrap();
    let _ = session.get_packets_to_send(now, 0);

    // 2. Force two retransmissions by receiving a NACK for fragments 0 and 1.
    use smallvec::smallvec;
    let nack = tox_sequenced::protocol::Nack {
        message_id: msg_id,
        missing_indices: smallvec![FragmentIndex(0), FragmentIndex(1)],
    };
    session.handle_packet(Packet::Nack(nack), now);

    // 3. Advance past pacing
    tp.advance(Duration::from_millis(100));
    let now_p = tp.now_instant();

    // 4. next_wakeup will return 'now_p' because has_retransmit is true and pacing is over.
    let wakeup = session.next_wakeup(now_p);
    assert_eq!(wakeup, now_p);

    // 5. get_packets_to_send sends ONE retransmission (bypassing cwnd because any_data_sent is false)
    // Then it tries to send the second retransmission. any_data_sent is true,
    // so it checks cwnd.
    let packets = session.get_packets_to_send(now_p, 100);
    assert!(!packets.is_empty());

    // 6. If packets.len() < 2, it means one retransmission is still in queue.
    // next_wakeup SHOULD NOT be 'now_p' if get_packets_to_send just finished and didn't send the rest.
    if packets.len() < 2 {
        let wakeup2 = session.next_wakeup(now_p);
        assert!(
            wakeup2 > now_p,
            "Tight loop! next_wakeup returned 'now' but retransmission is blocked by cwnd."
        );
    }
}

// end of file
