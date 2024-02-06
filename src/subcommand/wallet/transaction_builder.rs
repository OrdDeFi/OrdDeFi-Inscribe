//! Ordinal transaction construction is fraught.
//!
//! Ordinal-aware transaction construction has additional invariants,
//! constraints, and concerns in addition to those of normal, non-ordinal-aware
//! Bitcoin transactions.
//!
//! This module contains a `TransactionBuilder` struct that facilitates
//! constructing ordinal-aware transactions that take these additional
//! conditions into account.
//!
//! The external interface is `TransactionBuilder::new`, which returns a
//! constructed transaction given the `Target`, which include the outgoing sat
//! to send, the wallets current UTXOs and their sat ranges, and the
//! recipient's address. To build the transaction call
//! `Transaction::build_transaction`.
//!
//! `Target::Postage` ensures that the outgoing value is at most 20,000 sats,
//! reducing it to 10,000 sats if coin selection requires adding excess value.
//!
//! `Target::Value(Amount)` ensures that the outgoing value is exactly the
//! requested amount,
//!
//! Internally, `TransactionBuilder` calls multiple methods that implement
//! transformations responsible for individual concerns, such as ensuring that
//! the transaction fee is paid, and that outgoing outputs aren't too large.
//!
//! This module is heavily tested. For all features of transaction
//! construction, there should be a positive test that checks that the feature
//! is implemented correctly, an assertion in the final
//! `Transaction::build_transaction` method that the built transaction is
//! correct with respect to the feature, and a test that the assertion fires as
//! expected.

use {
  super::*,
  std::cmp::{max, min},
  bitcoin::blockdata::script::Builder,
};

#[derive(Debug, PartialEq)]
pub enum Error {
  DuplicateAddress(Address),
  Dust {
    output_value: Amount,
    dust_value: Amount,
  },
  NotEnoughCardinalUtxos,
  NotInWallet(SatPoint),
  OutOfRange(SatPoint, u64),
  UtxoContainsAdditionalInscription {
    outgoing_satpoint: SatPoint,
    inscribed_satpoint: SatPoint,
    inscription_id: InscriptionId,
  },
  ValueOverflow,
}

#[derive(Debug, PartialEq)]
pub enum Target {
  Value(Amount),
  Postage,
  ExactPostage(Amount),
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Error::Dust {
        output_value,
        dust_value,
      } => write!(f, "output value is below dust value: {output_value} < {dust_value}"),
      Error::NotInWallet(outgoing_satpoint) => write!(f, "outgoing satpoint {outgoing_satpoint} not in wallet"),
      Error::OutOfRange(outgoing_satpoint, maximum) => write!(f, "outgoing satpoint {outgoing_satpoint} offset higher than maximum {maximum}"),
      Error::NotEnoughCardinalUtxos => write!(
        f,
        "wallet does not contain enough cardinal UTXOs, please add additional funds to wallet."
      ),
      Error::UtxoContainsAdditionalInscription {
        outgoing_satpoint,
        inscribed_satpoint,
        inscription_id,
      } => write!(
        f,
        "cannot send {outgoing_satpoint} without also sending inscription {inscription_id} at {inscribed_satpoint}"
      ),
      Error::ValueOverflow => write!(f, "arithmetic overflow calculating value"),
      Error::DuplicateAddress(address) => write!(f, "duplicate input address: {address}"),
    }
  }
}

impl std::error::Error for Error {}

#[derive(Debug, PartialEq)]
pub struct TransactionBuilder {
  amounts: BTreeMap<OutPoint, Amount>,
  change_addresses: Address,
  fee_rate: FeeRate,
  inputs: Vec<OutPoint>,
  inscriptions: BTreeMap<SatPoint, InscriptionId>,
  locked_utxos: BTreeSet<OutPoint>,
  outgoing: SatPoint,
  outputs: Vec<(Address, Amount)>,
  recipient: Address,
  runic_utxos: BTreeSet<OutPoint>,
  target: Target,
  unused_change_addresses: Vec<Address>,
  utxos: BTreeSet<OutPoint>,
}

type Result<T> = std::result::Result<T, Error>;

impl TransactionBuilder {
  const ADDITIONAL_INPUT_VBYTES: usize = 58;
  const ADDITIONAL_OUTPUT_VBYTES: usize = 43;
  const SCHNORR_SIGNATURE_SIZE: usize = 64;
  pub(crate) const MAX_POSTAGE: Amount = Amount::from_sat(2 * 10_000);

  pub fn new(
    outgoing: SatPoint,
    inscriptions: BTreeMap<SatPoint, InscriptionId>,
    amounts: BTreeMap<OutPoint, Amount>,
    locked_utxos: BTreeSet<OutPoint>,
    runic_utxos: BTreeSet<OutPoint>,
    recipient: Address,
    change: Address,
    fee_rate: FeeRate,
    target: Target,
  ) -> Self {
    Self {
      utxos: amounts.keys().cloned().collect(),
      amounts,
      change_addresses: change.clone(),
      fee_rate,
      inputs: Vec::new(),
      inscriptions,
      locked_utxos,
      outgoing,
      outputs: Vec::new(),
      recipient,
      runic_utxos,
      target,
      unused_change_addresses: vec![change],
    }
  }

  pub fn build_transaction(self) -> Result<Transaction> {
    match self.target {
      Target::Value(output_value) | Target::ExactPostage(output_value) => {
        let dust_value = self.recipient.script_pubkey().dust_value();

        if output_value < dust_value {
          return Err(Error::Dust {
            output_value,
            dust_value,
          });
        }
      }
      _ => (),
    }

    self
      .select_outgoing()?
      .align_outgoing()
      .pad_alignment_output()?
      .add_value()?
      .strip_value()
      .deduct_fee()
      .build()
  }

  fn select_outgoing(mut self) -> Result<Self> {
    let dust_limit = self
      .unused_change_addresses
      .last()
      .unwrap()
      .script_pubkey()
      .dust_value()
      .to_sat();

    for (inscribed_satpoint, inscription_id) in self.inscriptions.iter().rev() {
      if self.outgoing.outpoint == inscribed_satpoint.outpoint
        && self.outgoing.offset != inscribed_satpoint.offset
        && self.outgoing.offset < inscribed_satpoint.offset + dust_limit
      {
        return Err(Error::UtxoContainsAdditionalInscription {
          outgoing_satpoint: self.outgoing,
          inscribed_satpoint: *inscribed_satpoint,
          inscription_id: *inscription_id,
        });
      }
    }

    let amount = *self
      .amounts
      .get(&self.outgoing.outpoint)
      .ok_or(Error::NotInWallet(self.outgoing))?;

    if self.outgoing.offset >= amount.to_sat() {
      return Err(Error::OutOfRange(self.outgoing, amount.to_sat() - 1));
    }

    self.utxos.remove(&self.outgoing.outpoint);
    self.inputs.push(self.outgoing.outpoint);
    self.outputs.push((self.recipient.clone(), amount));

    tprintln!(
      "selected outgoing outpoint {} with value {}",
      self.outgoing.outpoint,
      amount.to_sat()
    );

    Ok(self)
  }

  fn align_outgoing(mut self) -> Self {
    assert_eq!(self.outputs.len(), 1, "invariant: only one output");

    assert_eq!(
      self.outputs[0].0, self.recipient,
      "invariant: first output is recipient"
    );

    let sat_offset = self.calculate_sat_offset();

    if sat_offset == 0 {
      tprintln!("outgoing is aligned");
    } else {
      tprintln!("aligned outgoing with {sat_offset} sat padding output");
      self.outputs.insert(
        0,
        (
          self
            .unused_change_addresses
            .pop()
            .expect("not enough change addresses"),
          Amount::from_sat(sat_offset),
        ),
      );
      self.outputs.last_mut().expect("no output").1 -= Amount::from_sat(sat_offset);
    }

    self
  }

  fn pad_alignment_output(mut self) -> Result<Self> {
    if self.outputs[0].0 == self.recipient {
      tprintln!("no alignment output");
    } else {
      let dust_limit = self
        .unused_change_addresses
        .last()
        .unwrap()
        .script_pubkey()
        .dust_value();

      if self.outputs[0].1 >= dust_limit {
        tprintln!("no padding needed");
      } else {
        while self.outputs[0].1 < dust_limit {
          let (utxo, size) = self.select_cardinal_utxo(dust_limit - self.outputs[0].1, true)?;

          self.inputs.insert(0, utxo);
          self.outputs[0].1 += size;

          tprintln!(
            "padded alignment output to {} with additional {size} sat input",
            self.outputs[0].1
          );
        }
      }
    }

    Ok(self)
  }

  fn add_value(mut self) -> Result<Self> {
    let estimated_fee = self.estimate_fee();

    let min_value = match self.target {
      Target::Postage => self.outputs.last().unwrap().0.script_pubkey().dust_value(),
      Target::Value(value) | Target::ExactPostage(value) => value,
    };

    let total = min_value
      .checked_add(estimated_fee)
      .ok_or(Error::ValueOverflow)?;

    if let Some(mut deficit) = total.checked_sub(self.outputs.last().unwrap().1) {
      while deficit > Amount::ZERO {
        let additional_fee = self.fee_rate.fee(Self::ADDITIONAL_INPUT_VBYTES);

        let needed = deficit
          .checked_add(additional_fee)
          .ok_or(Error::ValueOverflow)?;

        let (utxo, value) = self.select_cardinal_utxo(needed, false)?;

        let benefit = value
          .checked_sub(additional_fee)
          .ok_or(Error::NotEnoughCardinalUtxos)?;

        self.inputs.push(utxo);

        self.outputs.last_mut().unwrap().1 += value;

        if benefit > deficit {
          tprintln!("added {value} sat input to cover {deficit} sat deficit");
          deficit = Amount::ZERO;
        } else {
          tprintln!("added {value} sat input to reduce {deficit} sat deficit by {benefit} sat");
          deficit -= benefit;
        }
      }
    }

    Ok(self)
  }

  fn strip_value(mut self) -> Self {
    let sat_offset = self.calculate_sat_offset();

    let total_output_amount = self
      .outputs
      .iter()
      .map(|(_address, amount)| *amount)
      .sum::<Amount>();

    self
      .outputs
      .iter()
      .find(|(address, _amount)| address == &self.recipient)
      .expect("couldn't find output that contains the index");

    let value = total_output_amount - Amount::from_sat(sat_offset);

    if let Some(excess) = value.checked_sub(self.fee_rate.fee(self.estimate_vbytes())) {
      let (max, target) = match self.target {
        Target::ExactPostage(postage) => (postage, postage),
        Target::Postage => (Self::MAX_POSTAGE, TARGET_POSTAGE),
        Target::Value(value) => (value, value),
      };

      if excess > max
        && value.checked_sub(target).unwrap()
          > self
            .unused_change_addresses
            .last()
            .unwrap()
            .script_pubkey()
            .dust_value()
            + self
              .fee_rate
              .fee(self.estimate_vbytes() + Self::ADDITIONAL_OUTPUT_VBYTES)
      {
        tprintln!("stripped {} sats", (value - target).to_sat());
        self.outputs.last_mut().expect("no outputs found").1 = target;
        self.outputs.push((
          self
            .unused_change_addresses
            .pop()
            .expect("not enough change addresses"),
          value - target,
        ));
      }
    }

    self
  }

  fn deduct_fee(mut self) -> Self {
    let sat_offset = self.calculate_sat_offset();

    let fee = self.estimate_fee();

    let total_output_amount = self
      .outputs
      .iter()
      .map(|(_address, amount)| *amount)
      .sum::<Amount>();

    let (_address, last_output_amount) = self
      .outputs
      .last_mut()
      .expect("No output to deduct fee from");

    assert!(
      total_output_amount.checked_sub(fee).unwrap() > Amount::from_sat(sat_offset),
      "invariant: deducting fee does not consume sat",
    );

    assert!(
      *last_output_amount >= fee,
      "invariant: last output can pay fee: {} {}",
      *last_output_amount,
      fee,
    );

    *last_output_amount -= fee;

    self
  }

  /// Estimate the size in virtual bytes of the transaction under construction.
  /// We initialize wallets with taproot descriptors only, so we know that all
  /// inputs are taproot key path spends, which allows us to know that witnesses
  /// will all consist of single Schnorr signatures.
  fn estimate_vbytes(&self) -> usize {
    Self::estimate_vbytes_with(
      self.inputs.len(),
      self
        .outputs
        .iter()
        .map(|(address, _amount)| address)
        .cloned()
        .collect(),
    )
  }

  fn estimate_vbytes_with(inputs: usize, outputs: Vec<Address>) -> usize {
    Transaction {
      version: 2,
      lock_time: LockTime::ZERO,
      input: (0..inputs)
        .map(|_| TxIn {
          previous_output: OutPoint::null(),
          script_sig: ScriptBuf::new(),
          sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
          witness: Witness::from_slice(&[&[0; Self::SCHNORR_SIGNATURE_SIZE]]),
        })
        .collect(),
      output: outputs
        .into_iter()
        .map(|address| TxOut {
          value: 0,
          script_pubkey: address.script_pubkey(),
        })
        .collect(),
    }
    .vsize()
  }

  fn estimate_fee(&self) -> Amount {
    self.fee_rate.fee(self.estimate_vbytes())
  }

  fn build(self) -> Result<Transaction> {
    let recipient = self.recipient.script_pubkey();
    let mut outputs: Vec<TxOut> = self
        .outputs
        .iter()
        .map(|(address, amount)| TxOut {
          value: amount.to_sat(),
          script_pubkey: address.script_pubkey(),
        })
        .collect();
    // append OrdDeFi auth OpReturn
    let data = b"orddefi:auth";
    let op_return_script = Builder::new()
        .push_opcode(opcodes::all::OP_RETURN)
        .push_slice(data)
        .into_script();
    let op_return_output = TxOut {
      value: 0,
      script_pubkey: op_return_script,
    };
    outputs.push(op_return_output);

    let transaction = Transaction {
      version: 2,
      lock_time: LockTime::ZERO,
      input: self
        .inputs
        .iter()
        .map(|outpoint| TxIn {
          previous_output: *outpoint,
          script_sig: ScriptBuf::new(),
          sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
          witness: Witness::new(),
        })
        .collect(),
      output: outputs,
    };

    assert_eq!(
      self
        .amounts
        .iter()
        .filter(|(outpoint, amount)| *outpoint == &self.outgoing.outpoint
          && self.outgoing.offset < amount.to_sat())
        .count(),
      1,
      "invariant: outgoing sat is contained in utxos"
    );

    assert_eq!(
      transaction
        .input
        .iter()
        .filter(|tx_in| tx_in.previous_output == self.outgoing.outpoint)
        .count(),
      1,
      "invariant: inputs spend outgoing sat"
    );

    let mut sat_offset = 0;
    let mut found = false;
    for tx_in in &transaction.input {
      if tx_in.previous_output == self.outgoing.outpoint {
        sat_offset += self.outgoing.offset;
        found = true;
        break;
      } else {
        sat_offset += self.amounts[&tx_in.previous_output].to_sat();
      }
    }
    assert!(found, "invariant: outgoing sat is found in inputs");

    let mut output_end = 0;
    let mut found = false;
    for tx_out in &transaction.output {
      output_end += tx_out.value;
      if output_end > sat_offset {
        assert_eq!(
          tx_out.script_pubkey, recipient,
          "invariant: outgoing sat is sent to recipient"
        );
        found = true;
        break;
      }
    }
    assert!(found, "invariant: outgoing sat is found in outputs");

    let mut offset = 0;
    for output in &transaction.output {
      if output.script_pubkey == self.recipient.script_pubkey() {
        let slop = self.fee_rate.fee(Self::ADDITIONAL_OUTPUT_VBYTES);

        match self.target {
          Target::Postage => {
            assert!(
              Amount::from_sat(output.value) <= Self::MAX_POSTAGE + slop,
              "invariant: excess postage is stripped"
            );
          }
          Target::ExactPostage(postage) => {
            assert!(
              Amount::from_sat(output.value) <= postage + slop,
              "invariant: excess postage is stripped"
            );
          }
          Target::Value(value) => {
          }
        }
      } else {
      }
      offset += output.value;
    }

    let mut actual_fee = Amount::ZERO;
    for input in &transaction.input {
      actual_fee += self.amounts[&input.previous_output];
    }
    for output in &transaction.output {
      actual_fee -= Amount::from_sat(output.value);
    }

    let mut modified_tx = transaction.clone();
    for input in &mut modified_tx.input {
      input.witness = Witness::from_slice(&[&[0; 64]]);
    }
    let expected_fee = self.fee_rate.fee(modified_tx.vsize());

    // assert_eq!(
    //   actual_fee, expected_fee,
    //   "invariant: fee estimation is correct",
    // );

    for tx_out in &transaction.output {
      assert!(
        Amount::from_sat(tx_out.value) >= tx_out.script_pubkey.dust_value(),
        "invariant: all outputs are above dust limit",
      );
    }

    Ok(transaction)
  }

  fn calculate_sat_offset(&self) -> u64 {
    let mut sat_offset = 0;
    for outpoint in &self.inputs {
      if *outpoint == self.outgoing.outpoint {
        return sat_offset + self.outgoing.offset;
      } else {
        sat_offset += self.amounts[outpoint].to_sat();
      }
    }

    panic!("Could not find outgoing sat in inputs");
  }

  /// Cardinal UTXOs are those that are unlocked, contain no inscriptions, and
  /// contain no runes, can therefore be used to pad transactions and pay fees.
  /// Sometimes multiple cardinal UTXOs are needed and depending on the context
  /// we want to select either ones above or under (when trying to consolidate
  /// dust outputs) the target value.
  fn select_cardinal_utxo(
    &mut self,
    target_value: Amount,
    prefer_under: bool,
  ) -> Result<(OutPoint, Amount)> {
    tprintln!(
      "looking for {} cardinal worth {target_value}",
      if prefer_under { "smaller" } else { "bigger" }
    );

    let inscribed_utxos = self
      .inscriptions
      .keys()
      .map(|satpoint| satpoint.outpoint)
      .collect::<BTreeSet<OutPoint>>();

    let mut best_match = None;
    for utxo in &self.utxos {
      if self.runic_utxos.contains(utxo)
        || inscribed_utxos.contains(utxo)
        || self.locked_utxos.contains(utxo)
      {
        continue;
      }

      let current_value = self.amounts[utxo];

      let (_, best_value) = match best_match {
        Some(prev) => prev,
        None => {
          best_match = Some((*utxo, current_value));
          (*utxo, current_value)
        }
      };

      let abs_diff = |a: Amount, b: Amount| -> Amount { max(a, b) - min(a, b) };
      let is_closer = abs_diff(current_value, target_value) < abs_diff(best_value, target_value);

      let not_preference_but_closer = if prefer_under {
        best_value > target_value && is_closer
      } else {
        best_value < target_value && is_closer
      };

      let is_preference_and_closer = if prefer_under {
        current_value <= target_value && is_closer
      } else {
        current_value >= target_value && is_closer
      };

      let newly_meets_preference = if prefer_under {
        best_value > target_value && current_value <= target_value
      } else {
        best_value < target_value && current_value >= target_value
      };

      if is_preference_and_closer || not_preference_but_closer || newly_meets_preference {
        best_match = Some((*utxo, current_value))
      }
    }

    let (utxo, value) = best_match.ok_or(Error::NotEnoughCardinalUtxos)?;

    self.utxos.remove(&utxo);
    tprintln!("found cardinal worth {}", value);

    Ok((utxo, value))
  }
}
