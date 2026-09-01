#[cfg(feature = "flavor-hyper")]
pub mod hyper;

#[cfg(feature = "flavor-tungstenite")]
pub mod hyper_tungstenite;

#[cfg(feature = "flavor-testing")]
pub mod testing;
