//! A notion of a cryptographic proof of a value in a Merkle tree.
use std::hash::{Hash, Hasher};
use std::cmp::Ordering;

use ring::digest::Algorithm;

use tree::Tree;
use hashutils::HashUtils;

/// An inclusion proof represent the fact that a `value` is a member
/// of a `MerkleTree` with root hash `root_hash`, and hash function `algorithm`.
#[cfg_attr(feature = "serialization-serde", derive(Serialize))]
#[derive(Clone, Debug)]
pub struct Proof<T> {
    /// The hashing algorithm used in the original `MerkleTree`
    #[cfg_attr(feature = "serialization-serde", serde(skip))]
    pub algorithm: &'static Algorithm,

    /// The hash of the root of the original `MerkleTree`
    pub root_hash: Vec<u8>,

    /// The first `Lemma` of the `Proof`
    pub lemma: Lemma,

    /// The value concerned by this `Proof`
    pub value: T,
}

impl<T: PartialEq> PartialEq for Proof<T> {
    fn eq(&self, other: &Proof<T>) -> bool {
        self.root_hash == other.root_hash && self.lemma == other.lemma && self.value == other.value
    }
}

impl<T: Eq> Eq for Proof<T> {}

impl<T: Ord> PartialOrd for Proof<T> {
    fn partial_cmp(&self, other: &Proof<T>) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: Ord> Ord for Proof<T> {
    fn cmp(&self, other: &Proof<T>) -> Ordering {
        self.root_hash
            .cmp(&other.root_hash)
            .then(self.value.cmp(&other.value))
            .then_with(|| self.lemma.cmp(&other.lemma))
    }
}

impl<T: Hash> Hash for Proof<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.root_hash.hash(state);
        self.lemma.hash(state);
        self.value.hash(state);
    }
}

impl<T> Proof<T> {
    /// Constructs a new `Proof`
    pub fn new(algo: &'static Algorithm, root_hash: Vec<u8>, lemma: Lemma, value: T) -> Self {
        Proof {
            algorithm: algo,
            root_hash: root_hash,
            lemma: lemma,
            value: value,
        }
    }

    /// Checks whether this inclusion proof is well-formed,
    /// and whether its root hash matches the given `root_hash`.
    pub fn validate(&self, root_hash: &[u8]) -> bool {
        if self.root_hash != root_hash || self.lemma.node_hash != root_hash {
            return false;
        }

        self.validate_lemma(&self.lemma)
    }

    fn validate_lemma(&self, lemma: &Lemma) -> bool {
        match lemma.sub_lemma {

            None => lemma.sibling_hash.is_none(),

            Some(ref sub) => {
                match lemma.sibling_hash {
                    None => false,

                    Some(Positioned::Left(ref hash)) => {
                        let combined = self.algorithm.hash_nodes(hash, &sub.node_hash);
                        let hashes_match = combined.as_ref() == lemma.node_hash.as_slice();
                        hashes_match && self.validate_lemma(sub)
                    }

                    Some(Positioned::Right(ref hash)) => {
                        let combined = self.algorithm.hash_nodes(&sub.node_hash, hash);
                        let hashes_match = combined.as_ref() == lemma.node_hash.as_slice();
                        hashes_match && self.validate_lemma(sub)
                    }

                }
            }
        }
    }

    /// Returns the proof data, omitting the algorithm.
    pub fn into_data(self) -> ProofData<T> {
        ProofData {
            root_hash: self.root_hash,
            lemma: self.lemma,
            value: self.value,
        }
    }
}

/// A proof without the `algorithm`, for easy serialization and deserialization.
#[cfg_attr(feature = "serialization-serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug)]
pub struct ProofData<T> {
    /// The hash of the root of the original `MerkleTree`
    pub root_hash: Vec<u8>,

    /// The first `Lemma` of the `Proof`
    pub lemma: Lemma,

    /// The value concerned by this `Proof`
    pub value: T,
}

impl<T> ProofData<T> {
    /// Returns the proof with this data and the given algorithm.
    pub fn into_proof(self, algorithm: &'static Algorithm) -> Proof<T> {
        Proof {
            algorithm,
            root_hash: self.root_hash,
            lemma: self.lemma,
            value: self.value,
        }
    }
}

/// A `Lemma` holds the hash of a node, the hash of its sibling node,
/// and a sub lemma, whose `node_hash`, when combined with this `sibling_hash`
/// must be equal to this `node_hash`.
#[cfg_attr(feature = "serialization-serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lemma {
    /// The hash of a node.
    pub node_hash: Vec<u8>,
    /// The hash of the child node under which the value NOT located. Also
    /// recorded in the type is the direction of the sibling from the lemma
    /// node. The value is consequently located in the other direction.
    pub sibling_hash: Option<Positioned<Vec<u8>>>,
    /// The hash of the child node under which the value IS located.
    pub sub_lemma: Option<Box<Lemma>>,
}

impl Lemma {
    /// Attempts to generate a proof that the a value with hash `needle` is a member of the given `tree`.
    pub fn new<T>(tree: &Tree<T>, needle: &[u8]) -> Option<Lemma> {
        match *tree {
            Tree::Empty { .. } => None,

            Tree::Leaf { ref hash, .. } => Lemma::new_leaf_proof(hash, needle),

            Tree::Node {
                ref hash,
                ref left,
                ref right,
            } => Lemma::new_tree_proof(hash, needle, left, right),
        }
    }

    fn new_leaf_proof(hash: &[u8], needle: &[u8]) -> Option<Lemma> {
        if *hash == *needle {
            Some(Lemma {
                node_hash: hash.into(),
                sibling_hash: None,
                sub_lemma: None,
            })
        } else {
            None
        }
    }

    fn new_tree_proof<T>(
        hash: &[u8],
        needle: &[u8],
        left: &Tree<T>,
        right: &Tree<T>,
    ) -> Option<Lemma> {
        Lemma::new(left, needle)
            .map(|lemma| {
                let right_hash = right.hash().clone();
                let sub_lemma = Some(Positioned::Right(right_hash));
                (lemma, sub_lemma)
            })
            .or_else(|| {
                let sub_lemma = Lemma::new(right, needle);
                sub_lemma.map(|lemma| {
                    let left_hash = left.hash().clone();
                    let sub_lemma = Some(Positioned::Left(left_hash));
                    (lemma, sub_lemma)
                })
            })
            .map(|(sub_lemma, sibling_hash)| {
                Lemma {
                    node_hash: hash.into(),
                    sibling_hash: sibling_hash,
                    sub_lemma: Some(Box::new(sub_lemma)),
                }
            })
    }
}

/// Tags a value so that we know from which branch of a `Tree` (if any) it was found.
#[cfg_attr(feature = "serialization-serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Positioned<T> {
    /// The value was found in the left branch
    Left(T),

    /// The value was found in the right branch
    Right(T),
}
