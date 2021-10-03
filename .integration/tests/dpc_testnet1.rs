// Copyright (C) 2019-2021 Aleo Systems Inc.
// This file is part of the snarkVM library.

// The snarkVM library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The snarkVM library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the snarkVM library. If not, see <https://www.gnu.org/licenses/>.

use snarkvm_algorithms::prelude::*;
use snarkvm_curves::bls12_377::{Fq, Fr};
use snarkvm_dpc::{prelude::*, testnet1::*};
use snarkvm_r1cs::{ConstraintSynthesizer, ConstraintSystem, TestConstraintSystem};
use snarkvm_utilities::{FromBytes, ToBytes, ToMinimalBits};

use chrono::Utc;
use rand::SeedableRng;
use rand_chacha::ChaChaRng;

#[test]
fn test_testnet1_inner_circuit_id_sanity_check() {
    let expected_inner_circuit_id = vec![
        71, 211, 100, 34, 123, 194, 23, 227, 47, 170, 213, 199, 169, 234, 0, 63, 120, 12, 153, 10, 129, 180, 193, 203,
        255, 244, 250, 69, 178, 106, 236, 246, 69, 128, 143, 176, 52, 162, 80, 64, 135, 119, 154, 19, 172, 142, 8, 0,
    ];
    let candidate_inner_circuit_id = <Testnet1 as Network>::inner_circuit_id().to_bytes_le().unwrap();
    assert_eq!(expected_inner_circuit_id, candidate_inner_circuit_id);
}

#[test]
fn dpc_testnet1_integration_test() {
    let mut rng = &mut ChaChaRng::seed_from_u64(1231275789u64);

    let mut ledger = Ledger::<Testnet1>::new().unwrap();
    assert_eq!(ledger.latest_block_height(), 0);
    assert_eq!(
        ledger.latest_block_hash(),
        Testnet1::genesis_block().to_block_hash().unwrap()
    );
    assert_eq!(&ledger.latest_block().unwrap(), Testnet1::genesis_block());
    assert_eq!((*ledger.latest_block_transactions().unwrap()).len(), 1);
    assert_eq!(
        ledger.latest_block().unwrap().to_coinbase_transaction().unwrap(),
        (*ledger.latest_block_transactions().unwrap())[0]
    );

    // Construct the previous block hash and new block height.
    let previous_block = ledger.latest_block().unwrap();
    let previous_hash = previous_block.to_block_hash().unwrap();
    let block_height = previous_block.header().height() + 1;
    assert_eq!(block_height, 1);

    // Construct the new block transactions.
    let recipient = Account::new(rng).unwrap();
    let amount = Block::<Testnet1>::block_reward(block_height);
    let coinbase_transaction = Transaction::<Testnet1>::new_coinbase(recipient.address(), amount, rng).unwrap();
    {
        // Check that the coinbase transaction is serialized and deserialized correctly.
        let transaction_bytes = coinbase_transaction.to_bytes_le().unwrap();
        let recovered_transaction = Transaction::<Testnet1>::read_le(&transaction_bytes[..]).unwrap();
        assert_eq!(coinbase_transaction, recovered_transaction);

        // Check that coinbase record can be decrypted from the transaction.
        let encrypted_record = &coinbase_transaction.encrypted_records()[0];
        let view_key = ViewKey::from_private_key(recipient.private_key()).unwrap();
        let decrypted_record = encrypted_record.decrypt(&view_key).unwrap();
        assert_eq!(decrypted_record.owner(), recipient.address());
        assert_eq!(decrypted_record.value() as i64, Block::<Testnet1>::block_reward(1).0);
    }
    let transactions = Transactions::from(&[coinbase_transaction]).unwrap();
    let transactions_root = transactions.to_transactions_root().unwrap();

    // Construct the new serial numbers root.
    let mut serial_numbers = SerialNumbers::<Testnet1>::new().unwrap();
    serial_numbers
        .add_all(previous_block.to_serial_numbers().unwrap())
        .unwrap();
    serial_numbers
        .add_all(transactions.to_serial_numbers().unwrap())
        .unwrap();
    let serial_numbers_root = serial_numbers.root();

    // Construct the new commitments root.
    let mut commitments = Commitments::<Testnet1>::new().unwrap();
    commitments.add_all(previous_block.to_commitments().unwrap()).unwrap();
    commitments.add_all(transactions.to_commitments().unwrap()).unwrap();
    let commitments_root = commitments.root();

    let timestamp = Utc::now().timestamp();
    let difficulty_target = Blocks::<Testnet1>::compute_difficulty_target(
        previous_block.timestamp(),
        previous_block.difficulty_target(),
        timestamp,
    );

    // Construct the new block header.
    let header = BlockHeader::new(
        block_height,
        timestamp,
        difficulty_target,
        transactions_root,
        serial_numbers_root,
        commitments_root,
        &mut rng,
    )
    .unwrap();

    // Construct the new block.
    let block = Block::from(previous_hash, header, transactions).unwrap();

    ledger.add_next_block(&block).unwrap();
    assert_eq!(ledger.latest_block_height(), 1);
}

#[test]
fn test_testnet1_dpc_execute_constraints() {
    let mut rng = ChaChaRng::seed_from_u64(1231275789u64);

    let recipient = Account::new(&mut rng).unwrap();
    let amount = AleoAmount::from_bytes(10 as i64);
    let state = StateTransition::builder()
        .add_output(Output::new(recipient.address, amount, Payload::default(), None).unwrap())
        .add_output(Output::new(recipient.address, amount, Payload::default(), None).unwrap())
        .build(&mut rng)
        .unwrap();

    let authorization = DPC::<Testnet1>::authorize(&vec![], &state, &mut rng).unwrap();

    // Generate the transaction ID.
    let transaction_id = authorization.to_transaction_id().unwrap();

    // Execute the program circuit.
    let execution = state
        .executable()
        .execute(PublicVariables::new(transaction_id))
        .unwrap();

    // Compute the encrypted records.
    let (_encrypted_records, encrypted_record_hashes, encrypted_record_randomizers) =
        authorization.to_encrypted_records(&mut rng).unwrap();

    let TransactionAuthorization {
        kernel,
        input_records,
        output_records,
        signatures,
    } = authorization;

    // Construct the ledger witnesses.
    let ledger_proof = LedgerProof::<Testnet1>::default();
    let ledger_digest = ledger_proof.commitments_root();
    let input_witnesses = ledger_proof.commitment_inclusion_proofs();

    //////////////////////////////////////////////////////////////////////////

    // Construct the inner circuit public and private variables.
    let inner_public_variables = InnerPublicVariables::new(
        kernel.to_transaction_id().unwrap(),
        &ledger_digest,
        &encrypted_record_hashes,
        Some(state.executable().program_id()),
    )
    .unwrap();
    let inner_private_variables = InnerPrivateVariables::new(
        &kernel,
        input_records.clone(),
        input_witnesses,
        signatures,
        output_records.clone(),
        encrypted_record_randomizers,
        state.executable(),
    )
    .unwrap();

    // Check that the core check constraint system was satisfied.
    let mut inner_circuit_cs = TestConstraintSystem::<Fr>::new();

    let inner_circuit = InnerCircuit::new(inner_public_variables.clone(), inner_private_variables);
    inner_circuit
        .generate_constraints(&mut inner_circuit_cs.ns(|| "Inner circuit"))
        .unwrap();

    if !inner_circuit_cs.is_satisfied() {
        println!("=========================================================");
        println!("Unsatisfied constraints:");
        println!("{}", inner_circuit_cs.which_is_unsatisfied().unwrap());
        println!("=========================================================");
    }

    println!("=========================================================");
    let num_constraints = inner_circuit_cs.num_constraints();
    println!("Inner circuit num constraints: {:?}", num_constraints);
    assert_eq!(176324, num_constraints);
    println!("=========================================================");

    assert!(inner_circuit_cs.is_satisfied());

    // Generate inner snark parameters and proof for verification in the outer snark
    let inner_snark_parameters = <Testnet1 as Network>::InnerSNARK::setup(
        &InnerCircuit::<Testnet1>::blank(),
        &mut SRS::CircuitSpecific(&mut rng),
    )
    .unwrap();

    let inner_snark_vk = inner_snark_parameters.1.clone();

    // NOTE: Do not change this to `Testnet1Parameters::inner_circuit_id()` as that will load the *saved* inner circuit VK.
    let inner_circuit_id = <Testnet1 as Network>::inner_circuit_id_crh()
        .hash_bits(&inner_snark_vk.to_minimal_bits())
        .unwrap();

    let inner_snark_proof =
        <Testnet1 as Network>::InnerSNARK::prove(&inner_snark_parameters.0, &inner_circuit, &mut rng).unwrap();

    // Verify that the inner circuit proof passes.
    assert!(
        <Testnet1 as Network>::InnerSNARK::verify(&inner_snark_vk, &inner_public_variables, &inner_snark_proof)
            .unwrap()
    );

    // Construct the outer circuit public and private variables.
    let outer_public_variables = OuterPublicVariables::new(&inner_public_variables, &inner_circuit_id);
    let outer_private_variables = OuterPrivateVariables::new(inner_snark_vk.clone(), inner_snark_proof, execution);

    // Check that the proof check constraint system was satisfied.
    let mut outer_circuit_cs = TestConstraintSystem::<Fq>::new();

    execute_outer_circuit::<Testnet1, _>(
        &mut outer_circuit_cs.ns(|| "Outer circuit"),
        &outer_public_variables,
        &outer_private_variables,
    )
    .unwrap();

    if !outer_circuit_cs.is_satisfied() {
        println!("=========================================================");
        println!(
            "Outer circuit num constraints: {:?}",
            outer_circuit_cs.num_constraints()
        );
        println!("Unsatisfied constraints:");
        println!("{}", outer_circuit_cs.which_is_unsatisfied().unwrap());
        println!("=========================================================");
    }

    println!("=========================================================");
    let num_constraints = outer_circuit_cs.num_constraints();
    println!("Outer circuit num constraints: {:?}", num_constraints);
    assert_eq!(162206, num_constraints);
    println!("=========================================================");

    assert!(outer_circuit_cs.is_satisfied());
}
