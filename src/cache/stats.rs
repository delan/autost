use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};

pub static STATS: Stats = Stats::new();

pub struct Stats {
    derivations_instantiated: AtomicUsize,
    derivations_realised: AtomicUsize,
    pending_derivation_writes: AtomicUsize,
    pending_output_writes: AtomicUsize,
    pending_write_logging_enabled: AtomicBool,
}

impl Stats {
    const fn new() -> Self {
        Self {
            derivations_instantiated: AtomicUsize::new(0),
            derivations_realised: AtomicUsize::new(0),
            pending_derivation_writes: AtomicUsize::new(0),
            pending_output_writes: AtomicUsize::new(0),
            pending_write_logging_enabled: AtomicBool::new(false),
        }
    }

    pub fn record_derivation_instantiated(&self) {
        eprint!(
            "\x1B[K... {} derivations instantiated, {} derivations realised\r",
            self.derivations_instantiated.fetch_add(1, SeqCst),
            self.derivations_realised.load(SeqCst)
        );
    }

    pub fn record_derivation_realised(&self) {
        eprint!(
            "\x1B[K... {} derivations instantiated, {} derivations realised\r",
            self.derivations_instantiated.load(SeqCst),
            self.derivations_realised.fetch_add(1, SeqCst)
        );
    }

    pub fn enable_pending_write_logging(&self) {
        self.pending_write_logging_enabled.store(true, SeqCst);
    }

    pub fn record_enqueue_output_write(&self) {
        self.pending_output_writes.fetch_add(1, SeqCst);
    }

    pub fn record_dequeue_output_write(&self) {
        if self.pending_write_logging_enabled.load(SeqCst) {
            eprint!(
                "\x1B[K... {} derivations pending, {} outputs pending\r",
                self.pending_derivation_writes.load(SeqCst),
                self.pending_output_writes.fetch_sub(1, SeqCst)
            );
        } else {
            self.pending_output_writes.fetch_sub(1, SeqCst);
        }
    }

    pub fn record_enqueue_derivation_write(&self) {
        self.pending_derivation_writes.fetch_add(1, SeqCst);
    }

    pub fn record_dequeue_derivation_write(&self) {
        if self.pending_write_logging_enabled.load(SeqCst) {
            eprint!(
                "\x1B[K... {} derivations pending, {} outputs pending\r",
                self.pending_derivation_writes.fetch_sub(1, SeqCst),
                self.pending_output_writes.load(SeqCst)
            );
        } else {
            self.pending_derivation_writes.fetch_sub(1, SeqCst);
        }
    }
}
