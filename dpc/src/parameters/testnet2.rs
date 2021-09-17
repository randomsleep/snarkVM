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

use crate::{
    account::ACCOUNT_ENCRYPTION_AND_SIGNATURE_INPUT,
    InnerPublicVariables,
    NoopProgram,
    OuterPublicVariables,
    Parameters,
    PublicVariables,
};
use snarkvm_algorithms::{
    commitment::{BHPCompressedCommitment, Blake2sCommitment},
    crh::BHPCompressedCRH,
    define_merkle_tree_parameters,
    encryption::ECIESPoseidonEncryption,
    prelude::*,
    prf::PoseidonPRF,
    signature::AleoSignatureScheme,
    snark::groth16::Groth16,
};
use snarkvm_curves::{
    bls12_377::Bls12_377,
    bw6_761::BW6_761,
    edwards_bls12::{
        EdwardsAffine as EdwardsBls12Affine,
        EdwardsParameters,
        EdwardsProjective as EdwardsBls12Projective,
    },
    edwards_bw6::EdwardsProjective as EdwardsBW6,
    traits::*,
};
use snarkvm_gadgets::{
    algorithms::{
        commitment::{BHPCompressedCommitmentGadget, Blake2sCommitmentGadget},
        crh::BHPCompressedCRHGadget,
        encryption::ECIESPoseidonEncryptionGadget,
        prf::PoseidonPRFGadget,
        signature::AleoSignatureSchemeGadget,
        snark::Groth16VerifierGadget,
    },
    curves::{bls12_377::PairingGadget, edwards_bls12::EdwardsBls12Gadget, edwards_bw6::EdwardsBW6Gadget},
};
use snarkvm_marlin::{
    constraints::{snark::MarlinSNARK, verifier::MarlinVerificationGadget},
    marlin::MarlinTestnet2Mode,
    FiatShamirAlgebraicSpongeRng,
    PoseidonSponge,
};
use snarkvm_parameters::{testnet2::*, Parameter};
use snarkvm_polycommit::sonic_pc::{sonic_kzg10::SonicKZG10Gadget, SonicKZG10};
use snarkvm_utilities::{FromBytes, ToMinimalBits};

use once_cell::sync::OnceCell;
use rand::{CryptoRng, Rng};
use std::{cell::RefCell, rc::Rc};

define_merkle_tree_parameters!(
    ProgramIDMerkleTreeParameters,
    <Testnet2Parameters as Parameters>::ProgramCircuitIDTreeCRH,
    8
);

define_merkle_tree_parameters!(
    CommitmentMerkleTreeParameters,
    <Testnet2Parameters as Parameters>::LedgerCommitmentsTreeCRH,
    32
);

define_merkle_tree_parameters!(
    SerialNumberMerkleTreeParameters,
    <Testnet2Parameters as Parameters>::LedgerSerialNumbersTreeCRH,
    32
);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Testnet2Parameters;

// TODO (raychu86): Optimize each of the window sizes in the type declarations below.
#[rustfmt::skip]
impl Parameters for Testnet2Parameters {
    const NETWORK_ID: u16 = 2u16;

    const NUM_INPUT_RECORDS: usize = 2;
    const NUM_OUTPUT_RECORDS: usize = 2;

    const MEMO_SIZE_IN_BYTES: usize = 64;

    type InnerCurve = Bls12_377;
    type InnerScalarField = <Self::InnerCurve as PairingEngine>::Fr;
    
    type OuterCurve = BW6_761;
    type OuterBaseField = <Self::OuterCurve as PairingEngine>::Fq;
    type OuterScalarField = <Self::OuterCurve as PairingEngine>::Fr;

    type ProgramAffineCurve = EdwardsBls12Affine;
    type ProgramAffineCurveGadget = EdwardsBls12Gadget;
    type ProgramProjectiveCurve = EdwardsBls12Projective;
    type ProgramCurveParameters = EdwardsParameters;
    type ProgramBaseField = <Self::ProgramCurveParameters as ModelParameters>::BaseField;
    type ProgramScalarField = <Self::ProgramCurveParameters as ModelParameters>::ScalarField;

    type InnerSNARK = Groth16<Self::InnerCurve, InnerPublicVariables<Testnet2Parameters>>;
    type InnerSNARKGadget = Groth16VerifierGadget<Self::InnerCurve, PairingGadget>;

    type OuterSNARK = Groth16<Self::OuterCurve, OuterPublicVariables<Testnet2Parameters>>;

    type ProgramSNARK = MarlinSNARK<
        Self::InnerScalarField,
        Self::OuterScalarField,
        SonicKZG10<Self::InnerCurve>,
        FiatShamirAlgebraicSpongeRng<Self::InnerScalarField, Self::OuterScalarField, PoseidonSponge<Self::OuterScalarField>>,
        MarlinTestnet2Mode,
        PublicVariables<Self>,
    >;
    type ProgramSNARKGadget = MarlinVerificationGadget<
        Self::InnerScalarField,
        Self::OuterScalarField,
        SonicKZG10<Self::InnerCurve>,
        SonicKZG10Gadget<Self::InnerCurve, Self::OuterCurve, PairingGadget>,
    >;

    type AccountEncryptionScheme = ECIESPoseidonEncryption<Self::ProgramCurveParameters>;
    type AccountEncryptionGadget = ECIESPoseidonEncryptionGadget<Self::ProgramCurveParameters, Self::InnerScalarField>;

    type AccountPRF = PoseidonPRF<Self::ProgramScalarField, 4, false>;
    type AccountSeed = <Self::AccountPRF as PRF>::Seed;
    
    type AccountSignatureScheme = AleoSignatureScheme<Self::ProgramCurveParameters>;
    type AccountSignatureGadget = AleoSignatureSchemeGadget<Self::ProgramCurveParameters, Self::InnerScalarField>;
    type AccountSignaturePublicKey = <Self::AccountSignatureScheme as SignatureScheme>::PublicKey;
    type AccountSignature = <Self::AccountSignatureScheme as SignatureScheme>::Signature;

    type EncryptedRecordCRH = BHPCompressedCRH<Self::ProgramProjectiveCurve, 80, 32>;
    type EncryptedRecordCRHGadget = BHPCompressedCRHGadget<Self::ProgramProjectiveCurve, Self::InnerScalarField, Self::ProgramAffineCurveGadget, 80, 32>;
    type EncryptedRecordDigest = <Self::EncryptedRecordCRH as CRH>::Output;

    type InnerCircuitIDCRH = BHPCompressedCRH<EdwardsBW6, 296, 32>;
    type InnerCircuitIDCRHGadget = BHPCompressedCRHGadget<EdwardsBW6, Self::OuterScalarField, EdwardsBW6Gadget, 296, 32>;
    type InnerCircuitID = <Self::InnerCircuitIDCRH as CRH>::Output;

    type LedgerCommitmentsTreeCRH = BHPCompressedCRH<Self::ProgramProjectiveCurve, 16, 32>;
    type LedgerCommitmentsTreeCRHGadget = BHPCompressedCRHGadget<Self::ProgramProjectiveCurve, Self::InnerScalarField, Self::ProgramAffineCurveGadget, 16, 32>;
    type LedgerCommitmentsTreeDigest = <Self::LedgerCommitmentsTreeCRH as CRH>::Output;
    type LedgerCommitmentsTreeParameters = CommitmentMerkleTreeParameters;

    type LedgerSerialNumbersTreeCRH = BHPCompressedCRH<Self::ProgramProjectiveCurve, 16, 32>;
    type LedgerSerialNumbersTreeDigest = <Self::LedgerSerialNumbersTreeCRH as CRH>::Output;
    type LedgerSerialNumbersTreeParameters = SerialNumberMerkleTreeParameters;

    type LocalDataCommitmentScheme = BHPCompressedCommitment<Self::ProgramProjectiveCurve, 24, 62>;
    type LocalDataCommitmentGadget = BHPCompressedCommitmentGadget<Self::ProgramProjectiveCurve, Self::InnerScalarField, Self::ProgramAffineCurveGadget, 24, 62>;
    type LocalDataCRH = BHPCompressedCRH<Self::ProgramProjectiveCurve, 16, 32>;
    type LocalDataCRHGadget = BHPCompressedCRHGadget<Self::ProgramProjectiveCurve, Self::InnerScalarField, Self::ProgramAffineCurveGadget, 16, 32>;
    type LocalDataRoot = <Self::LocalDataCRH as CRH>::Output;

    type ProgramCommitmentScheme = Blake2sCommitment;
    type ProgramCommitmentGadget = Blake2sCommitmentGadget;
    type ProgramCommitment = <Self::ProgramCommitmentScheme as CommitmentScheme>::Output;

    type ProgramCircuitIDCRH = BHPCompressedCRH<EdwardsBW6, 237, 16>;
    type ProgramCircuitIDCRHGadget = BHPCompressedCRHGadget<EdwardsBW6, Self::OuterScalarField, EdwardsBW6Gadget, 237, 16>;
    type ProgramCircuitID = <Self::ProgramCircuitIDCRH as CRH>::Output;

    type ProgramCircuitIDTreeCRH = BHPCompressedCRH<EdwardsBW6, 48, 16>;
    type ProgramCircuitIDTreeCRHGadget = BHPCompressedCRHGadget<EdwardsBW6, Self::OuterScalarField, EdwardsBW6Gadget, 48, 16>;
    type ProgramCircuitIDTreeDigest = <Self::ProgramCircuitIDTreeCRH as CRH>::Output;
    type ProgramCircuitTreeParameters = ProgramIDMerkleTreeParameters;
    
    type RecordCommitmentScheme = BHPCompressedCommitment<Self::ProgramProjectiveCurve, 48, 50>;
    type RecordCommitmentGadget = BHPCompressedCommitmentGadget<Self::ProgramProjectiveCurve, Self::InnerScalarField, Self::ProgramAffineCurveGadget, 48, 50>;
    type RecordCommitment = <Self::RecordCommitmentScheme as CommitmentScheme>::Output;

    type SerialNumberNonceCRH = BHPCompressedCRH<Self::ProgramProjectiveCurve, 32, 63>;
    type SerialNumberNonceCRHGadget = BHPCompressedCRHGadget<Self::ProgramProjectiveCurve, Self::InnerScalarField, Self::ProgramAffineCurveGadget, 32, 63>;
    type SerialNumberNonce = <Self::SerialNumberNonceCRH as CRH>::Output;

    type SerialNumberPRF = PoseidonPRF<Self::InnerScalarField, 4, false>;
    type SerialNumberPRFGadget = PoseidonPRFGadget<Self::InnerScalarField, 4, false>;
    type SerialNumber = <Self::SerialNumberPRF as PRF>::Output;

    type TransactionIDCRH = BHPCompressedCRH<Self::ProgramProjectiveCurve, 26, 62>;
    type TransactionID = <Self::TransactionIDCRH as CRH>::Output;

    dpc_setup!{Testnet2Parameters, account_encryption_scheme, AccountEncryptionScheme, ACCOUNT_ENCRYPTION_AND_SIGNATURE_INPUT}
    dpc_setup!{Testnet2Parameters, account_signature_scheme, AccountSignatureScheme, ACCOUNT_ENCRYPTION_AND_SIGNATURE_INPUT}
    dpc_setup!{Testnet2Parameters, encrypted_record_crh, EncryptedRecordCRH, "AleoEncryptedRecordCRH0"}
    dpc_setup!{Testnet2Parameters, inner_circuit_id_crh, InnerCircuitIDCRH, "AleoInnerCircuitIDCRH0"}
    dpc_setup!{Testnet2Parameters, local_data_commitment_scheme, LocalDataCommitmentScheme, "AleoLocalDataCommitmentScheme0"}
    dpc_setup!{Testnet2Parameters, local_data_crh, LocalDataCRH, "AleoLocalDataCRH0"}
    dpc_setup!{Testnet2Parameters, program_commitment_scheme, ProgramCommitmentScheme, "AleoProgramCommitmentScheme0"}
    dpc_setup!{Testnet2Parameters, program_circuit_id_crh, ProgramCircuitIDCRH, "AleoProgramCircuitIDCRH0"}
    dpc_setup!{Testnet2Parameters, program_circuit_id_tree_crh, ProgramCircuitIDTreeCRH, "AleoProgramCircuitIDTreeCRH0"}
    dpc_setup!{Testnet2Parameters, record_commitment_scheme, RecordCommitmentScheme, "AleoRecordCommitmentScheme0"}
    dpc_setup!{Testnet2Parameters, serial_number_nonce_crh, SerialNumberNonceCRH, "AleoSerialNumberNonceCRH0"}
    dpc_setup!{Testnet2Parameters, transaction_id_crh, TransactionIDCRH, "AleoTransactionIDCRH0"}

    fn inner_circuit_id() -> &'static Self::InnerCircuitID {
        static INNER_CIRCUIT_ID: OnceCell<<Testnet2Parameters as Parameters>::InnerCircuitID> = OnceCell::new();
        INNER_CIRCUIT_ID.get_or_init(|| Self::inner_circuit_id_crh()
            .hash_bits(&Self::inner_circuit_verifying_key().to_minimal_bits())
            .expect("Failed to hash inner circuit verifying key elements"))
    }

    dpc_snark_setup_with_mode!{Testnet2Parameters, inner_circuit_proving_key, InnerSNARK, ProvingKey, InnerSNARKPKParameters, "inner circuit proving key"}
    dpc_snark_setup!{Testnet2Parameters, inner_circuit_verifying_key, InnerSNARK, VerifyingKey, InnerSNARKVKParameters, "inner circuit verifying key"}
    
    fn noop_program() -> &'static NoopProgram<Self> {
        static NOOP_PROGRAM: OnceCell<NoopProgram<Testnet2Parameters>> = OnceCell::new();
        NOOP_PROGRAM.get_or_init(|| NoopProgram::<Testnet2Parameters>::load().expect("Failed to fetch the noop program"))
    }

    fn noop_circuit_id() -> &'static Self::ProgramCircuitID {
        static NOOP_CIRCUIT_ID: OnceCell<<Testnet2Parameters as Parameters>::ProgramCircuitID> = OnceCell::new();
        NOOP_CIRCUIT_ID.get_or_init(|| Self::program_circuit_id(Self::noop_circuit_verifying_key()).expect("Failed to hash noop circuit verifying key"))
    }
    
    dpc_snark_setup!{Testnet2Parameters, noop_circuit_proving_key, ProgramSNARK, ProvingKey, NoopProgramSNARKPKParameters, "noop circuit proving key"}
    dpc_snark_setup!{Testnet2Parameters, noop_circuit_verifying_key, ProgramSNARK, VerifyingKey, NoopProgramSNARKVKParameters, "noop circuit verifying key"}

    dpc_snark_setup_with_mode!{Testnet2Parameters, outer_circuit_proving_key, OuterSNARK, ProvingKey, OuterSNARKPKParameters, "outer circuit proving key"}
    dpc_snark_setup!{Testnet2Parameters, outer_circuit_verifying_key, OuterSNARK, VerifyingKey, OuterSNARKVKParameters, "outer circuit verifying key"}

    fn program_circuit_tree_parameters() -> &'static Self::ProgramCircuitTreeParameters {
        static PROGRAM_ID_TREE_PARAMETERS: OnceCell<<Testnet2Parameters as Parameters>::ProgramCircuitTreeParameters> = OnceCell::new();
        PROGRAM_ID_TREE_PARAMETERS.get_or_init(|| Self::ProgramCircuitTreeParameters::from(Self::program_circuit_id_tree_crh().clone()))
    }

    dpc_setup!{Testnet2Parameters, ledger_commitments_tree_crh, LedgerCommitmentsTreeCRH, "AleoLedgerCommitmentsTreeCRH0"}
    fn ledger_commitments_tree_parameters() -> &'static Self::LedgerCommitmentsTreeParameters {
        static LEDGER_COMMITMENTS_TREE_PARAMETERS: OnceCell<<Testnet2Parameters as Parameters>::LedgerCommitmentsTreeParameters> = OnceCell::new();
        LEDGER_COMMITMENTS_TREE_PARAMETERS.get_or_init(|| Self::LedgerCommitmentsTreeParameters::from(Self::ledger_commitments_tree_crh().clone()))
    }

    dpc_setup!{Testnet2Parameters, ledger_serial_numbers_tree_crh, LedgerSerialNumbersTreeCRH, "AleoLedgerSerialNumbersTreeCRH0"}
    fn ledger_serial_numbers_tree_parameters() -> &'static Self::LedgerSerialNumbersTreeParameters {
        static LEDGER_SERIAL_NUMBERS_TREE_PARAMETERS: OnceCell<<Testnet2Parameters as Parameters>::LedgerSerialNumbersTreeParameters> = OnceCell::new();
        LEDGER_SERIAL_NUMBERS_TREE_PARAMETERS.get_or_init(|| Self::LedgerSerialNumbersTreeParameters::from(Self::ledger_serial_numbers_tree_crh().clone()))
    }

    /// Returns the program SRS for Aleo applications.
    fn program_srs<R: Rng + CryptoRng>(_rng: &mut R) -> Rc<RefCell<SRS<R, <Self::ProgramSNARK as SNARK>::UniversalSetupParameters>>> {
        static UNIVERSAL_SRS: OnceCell<<<Testnet2Parameters as Parameters>::ProgramSNARK as SNARK>::UniversalSetupParameters> = OnceCell::new();
        let universal_srs = UNIVERSAL_SRS.get_or_init(|| <Self::ProgramSNARK as SNARK>::UniversalSetupParameters::from_bytes_le(
            &UniversalSRSParameters::load_bytes().expect("Failed to load universal SRS bytes"),
        ).unwrap());
        Rc::new(RefCell::new(SRS::<_, _>::Universal(universal_srs)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inner_circuit_sanity_check() {
        // Verify the inner circuit verifying key matches the one derived from the inner circuit proving key.
        assert_eq!(
            Testnet2Parameters::inner_circuit_verifying_key(),
            &Testnet2Parameters::inner_circuit_proving_key(true)
                .as_ref()
                .expect("Failed to load inner circuit proving key")
                .vk,
            "The inner circuit verifying key does not correspond to the inner circuit proving key"
        );
    }

    #[test]
    fn test_inner_circuit_id_derivation() {
        // Verify the inner circuit ID matches the one derived from the inner circuit verifying key.
        assert_eq!(
            Testnet2Parameters::inner_circuit_id(),
            &Testnet2Parameters::inner_circuit_id_crh()
                .hash_bits(&Testnet2Parameters::inner_circuit_verifying_key().to_minimal_bits())
                .expect("Failed to hash inner circuit ID"),
            "The inner circuit ID does not correspond to the inner circuit verifying key"
        );
    }

    #[test]
    fn test_outer_circuit_sanity_check() {
        // Verify the outer circuit verifying key matches the one derived from the outer circuit proving key.
        assert_eq!(
            Testnet2Parameters::outer_circuit_verifying_key(),
            &Testnet2Parameters::outer_circuit_proving_key(true)
                .as_ref()
                .expect("Failed to load outer circuit proving key")
                .vk,
            "The outer circuit verifying key does not correspond to the outer circuit proving key"
        );
    }
}
