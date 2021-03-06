// LNP/BP Rust Library
// Written in 2020 by
//     Dr. Maxim Orlovsky <orlovsky@pandoracore.com>
//
// To the extent possible under law, the author(s) have dedicated all
// copyright and related and neighboring rights to this software to
// the public domain worldwide. This software is distributed without
// any warranty.
//
// You should have received a copy of the MIT License
// along with this software.
// If not, see <https://opensource.org/licenses/MIT>.

//! # Bitcoin script types
//!
//! Bitcoin doesn't make a distinction between Bitcoin script coming from different sources, like
//! *scriptPubKey* in transaction output or witness and *sigScript* in transaction input. There are
//! many other possible script containers for Bitcoin script: redeem script, witness script,
//! tapscript. In fact, any "script" of `Script` type can be used for inputs and outputs.
//! What is a valid script for one will be a valid script for the other; the only req. is formatting
//! of opcodes & pushes. That would mean that in principle every input script can be used as an
//! output script, btu not vice versa. But really what makes a "script" is just the fact that it's
//! formatted correctly.
//!
//! While all `Script`s represent the same same type **semantically**, there is a clear distinction
//! at the **logical** level: Bitcoin script has the property to be committed into some other
//! Bitcoin script – in a nested structures like in several layers, like *redeemScript* inside of
//! *sigScript* used for P2SH, or *tapScript* within *witnessScript* coming from *witness* field
//! for Taproot. These nested layers do distinguish on the information they contain, since some of
//! them only commit to the hashes of the nested scripts (`ScriptHash`, `WitnessProgramm`) or
//! public keys (`PubkeyHash`, `WPubkeyHash`), while other contain the full source of the script.
//!
//! The present type system represents a solution to the problem: it distinguish different logical
//! types by introducing `Script` wrapper types. It defines `LockScript` as bottom layer or a script
//! hierarchy, containing no other script commitments (in form of their hashes). It also defines
//! types above on it: `PubkeyScript` (for whatever is there in `pubkeyScript` field of a `TxOut`),
//! `SigScript` (for whatever comes from `sigScript` field of `TxIn`), `RedeemScript` and `TapScript`.
//! Then, there are conversion functions, which for instance can analyse `PubkeyScript`
//! and if it is a custom script or P2PK return a `LockScript` type - or otherwise fail with the
//! error. So with this type system one is always sure which logical information it does contain.
//!
//! The following charts represent possible relations between script types:
//!
//! ```text
//!                                                                            LockScript
//!                                                                _________________________________
//!                                                                ^      ^  ^    ^                ^
//!                                                                |      |  |    |                |
//! [txout.scriptPubKey] <===> PubkeyScript --?--/P2PK & custom/---+      |  |    |                |
//!                                                                       |  |    |                |
//! [txin.sigScript] <===> SigScript --+--?!--/P2(W)PKH/--(#=PubkeyHash)--+  |    |                |
//!                                    |                                     |    |                |
//!                                    |                           (#=ScriptHash) |                |
//!                                    |                                     |    |                |
//!                                    +--?!--> RedeemScript --+--?!------/P2SH/  |                |
//!                                                            |                  |                |
//!                                                  /P2WSH-in-P2SH/  /#=V0_WitnessProgram_P2WSH/  |
//!                                                            |                  |                |
//!                                                            +--?!--> WitnessScript              |
//!                                                                       ^^      |                |
//!                                                                       || /#=V1_WitnessProgram/ |
//!                                                                       ||      |                |
//! [?txin.witness] <=====================================================++      +--?---> TapScript
//!
//! ```
//! Legend:
//! * `[source] <===> `: data source
//! * `[?source] <===> `: data source which may be absent
//! * `--+--`: algorithmic branching (alternative computation options)
//! * `--?-->`: a conversion exists, but it may fail (returns `Option` or `Result`)
//! * `--?!-->`: a conversion exists, but it may fail; however one of alternative branches must
//!              always succeed
//! * `----->`: a conversion exists which can't fail
//! * `--/format/--`: a format implied by scriptPubKey program
//! * `--(#=type)--`: the hash of the value following `->` must match to the value of the `<type>`
//!

use bitcoin::{hash_types::*, blockdata::script::*, secp256k1};
use miniscript::{Miniscript, MiniscriptKey, miniscript::iter::PubkeyOrHash};
use crate::Wrapper;


// STYLE: Multiline string literals with `"""` are still not supported for the meta-type macro tokens
wrapper!(LockScript, _LockScriptPhantom, Script, doc="\
    The deepest nested version of Bitcoin script containing no hashes of other scripts, including \
    P2SH redeemScript hashes or witnessProgramm (hash or wintness script), or public keys");
wrapper!(PubkeyScript, _PubkeyScriptPhantom, Script, doc="\
    A content of `scriptPubkey` from a transaction output");
wrapper!(SigScript, _SigScriptPhantom, Script, doc="\
    A content of `sigScript` from a transaction input");
wrapper!(WitnessScript, _WitnessScriptPhantom, Script, doc="\
    A part of the `witness` field from a transaction input according to BIP-141");
wrapper!(RedeemScript, _RedeemScriptPhantom, Script, doc="\
    redeemScript part of the witness program or sigScript which is hashed for P2(W)SH output");
wrapper!(TapScript, _TapScriptPhantom, Script, doc="\
    Any valid branch of Tapscript (BIP-342)");


#[derive(Debug)]
pub enum LockScriptParseError<Pk: MiniscriptKey> {
    PubkeyHash(Pk::Hash),
    Miniscript(miniscript::Error)
}

impl<Pk: MiniscriptKey> From<miniscript::Error> for LockScriptParseError<Pk> {
    fn from(miniscript_error: miniscript::Error) -> Self {
        Self::Miniscript(miniscript_error)
    }
}

impl LockScript {
    pub fn extract_pubkeys(&self) -> Result<Vec<secp256k1::PublicKey>, LockScriptParseError<bitcoin::PublicKey>> {
        Miniscript::parse(&self.clone().into_inner())?
            .iter_pubkeys_and_hashes()
            .try_fold(Vec::<secp256k1::PublicKey>::new(), |mut keys, item| match item {
                PubkeyOrHash::HashedPubkey(hash) => Err(LockScriptParseError::PubkeyHash(hash)),
                PubkeyOrHash::PlainPubkey(key) => {
                    keys.push(key.key);
                    Ok(keys)
                },
            })
    }

    pub fn replace_pubkeys(
        &self, processor: impl Fn(secp256k1::PublicKey) -> Option<secp256k1::PublicKey>
    ) -> Result<Self, LockScriptParseError<bitcoin::PublicKey>> {
        let result = Miniscript::parse(&self.clone().into_inner())?
            .replace_pubkeys_and_hashes(&|item: PubkeyOrHash<bitcoin::PublicKey>| {
                match item {
                    PubkeyOrHash::PlainPubkey(pubkey) =>
                        processor(pubkey.key)
                            .map(|key| PubkeyOrHash::PlainPubkey(bitcoin::PublicKey{compressed: true, key})),
                    PubkeyOrHash::HashedPubkey(_) => None,
                }
            })?;
        Ok(LockScript::from_inner(result.encode()))
    }
}


pub enum PubkeyScriptType {
    P2S(PubkeyScript),
    P2PK(bitcoin::PublicKey),
    P2PKH(PubkeyHash),
    P2SH(ScriptHash),
    P2OR(Vec<u8>),
    P2WPKH(WPubkeyHash),
    P2WSH(WScriptHash),
    P2TR(secp256k1::PublicKey),
}

pub enum PubkeyScriptSource {
    P2S(LockScript),
    P2PK(bitcoin::PublicKey),
    P2PKH(bitcoin::PublicKey),
    P2SH(LockScript),
    P2OR(Vec<u8>),
    P2WPKH(LockScript),
    P2WSH(LockScript),
    P2TR(bitcoin::PublicKey, TapScript),
}

impl From<Script> for PubkeyScriptType {
    fn from(script_pubkey: Script) -> Self {
        Self::P2S(PubkeyScript::from_inner(script_pubkey))
    }
}

impl From<PubkeyScriptType> for PubkeyScript {
    fn from(spkt: PubkeyScriptType) -> PubkeyScript {
        use PubkeyScriptType::*;

        PubkeyScript::from_inner(match spkt {
            P2S(script) => script.into_inner(),
            P2PK(pubkey) =>
                Builder::gen_p2pk(&pubkey).into_script(),
            P2PKH(pubkey_hash) => Builder::gen_p2pkh(&pubkey_hash).into_script(),
            P2SH(script_hash) => Builder::gen_p2sh(&script_hash).into_script(),
            P2OR(data) => Builder::gen_op_return(&data).into_script(),
            P2WPKH(wpubkey_hash) => Builder::gen_v0_p2wpkh(&wpubkey_hash).into_script(),
            P2WSH(wscript_hash) => Builder::gen_v0_p2wsh(&wscript_hash).into_script(),
            P2TR(pubkey) => unimplemented!(),
        })
    }
}