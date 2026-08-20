//! OP_RETURN output stubbing for archival persistence.

use blvm_protocol::Transaction;

const OP_RETURN: u8 = 0x6a;

pub fn is_op_return_script(script: &[u8]) -> bool {
    !script.is_empty() && script[0] == OP_RETURN
}

/// Stub OP_RETURN outputs to `[0x6a]`. Returns count of stubbed outputs.
pub fn stub_op_return_outputs(tx: &mut Transaction) -> usize {
    let mut count = 0;
    for out in tx.outputs.iter_mut() {
        if !is_op_return_script(&out.script_pubkey) {
            continue;
        }
        out.script_pubkey = vec![OP_RETURN];
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use blvm_protocol::{OutPoint, TransactionInput, TransactionOutput};

    fn opreturn_tx() -> Transaction {
        Transaction {
            version: 1,
            inputs: vec![TransactionInput {
                prevout: OutPoint {
                    hash: [1; 32],
                    index: 0,
                },
                script_sig: vec![],
                sequence: 0xffffffff,
            }]
            .into(),
            outputs: vec![
                TransactionOutput {
                    value: 0,
                    script_pubkey: vec![0x6a, 0x04, 0x01, 0x02, 0x03, 0x04],
                },
                TransactionOutput {
                    value: 1000,
                    script_pubkey: vec![0x51],
                },
            ]
            .into(),
            lock_time: 0,
        }
    }

    #[test]
    fn stubs_op_return_outputs() {
        let mut tx = opreturn_tx();
        let count = stub_op_return_outputs(&mut tx);
        assert_eq!(count, 1);
        assert_eq!(tx.outputs[0].script_pubkey, vec![0x6a]);
        assert_eq!(tx.outputs[1].script_pubkey, vec![0x51]);
    }
}
