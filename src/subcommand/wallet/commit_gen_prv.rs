use bitcoin::key::UntweakedKeyPair;
use super::*;

#[derive(Serialize, Deserialize)]
pub struct Output {
    pub xprv: Option<String>,
}

#[derive(Debug, Parser)]
pub(crate) struct CommitGenPrv {
}

impl CommitGenPrv {

    pub(crate) fn run() -> SubcommandResult {
        let secp256k1 = Secp256k1::new();
        let mut key_pair = UntweakedKeyPair::new(&secp256k1, &mut rand::thread_rng());

        let serialized_key = key_pair.secret_bytes();
        let hex_key = hex::encode(serialized_key);

        Ok(Box::new(crate::subcommand::wallet::commit_gen_prv::Output {
            xprv: Option::from(hex_key),
        }))
    }
}
