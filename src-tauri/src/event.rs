//! Bus d'événements applicatifs.
//!
//! Les jetons, changements de tour et avancements de plan avaient jusqu'ici
//! une seule destination : la fenêtre Tauri. Un client distant doit recevoir
//! exactement les mêmes, ce qui suppose que le cœur n'écrive plus directement
//! dans le webview.
//!
//! Tout passe donc par un bus de diffusion. Deux abonnés le consomment :
//! le pont Tauri, qui réémet vers la fenêtre locale, et le serveur distant,
//! qui pousse vers les WebSockets connectées. Ajouter un transport revient à
//! ajouter un abonné, sans toucher au cœur.
//!
//! Le canal est **borné**. Un abonné trop lent (fenêtre gelée, WebSocket
//! saturée) perd les événements les plus anciens plutôt que de faire enfler la
//! mémoire indéfiniment : sur un flux de jetons, retarder tout le monde serait
//! pire que sauter des fragments chez le seul retardataire.

use serde::Serialize;
use tokio::sync::broadcast;
use tracing::warn;

/// Profondeur du canal, en événements. Un flux de jetons rapide en produit
/// quelques centaines par seconde ; cette marge absorbe une fenêtre qui
/// hoquette sans conserver tout un historique.
const CAPACITE: usize = 1024;

/// Un événement diffusé : un nom et sa charge utile déjà sérialisée.
#[derive(Debug, Clone, Serialize)]
pub struct AppEvent {
    pub name: String,
    pub payload: serde_json::Value,
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<AppEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(CAPACITE);
        Self { tx }
    }

    /// Diffuse un événement à tous les abonnés.
    ///
    /// L'absence d'abonné n'est pas une erreur : au démarrage, ou quand la
    /// fenêtre est fermée alors qu'un traitement continue, il n'y a
    /// simplement personne à qui parler.
    pub fn emit<T: Serialize>(&self, name: &str, payload: T) {
        match serde_json::to_value(payload) {
            Ok(payload) => {
                let _ = self.tx.send(AppEvent {
                    name: name.to_string(),
                    payload,
                });
            }
            Err(e) => warn!(event = name, %e, "événement non sérialisable, ignoré"),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.tx.subscribe()
    }

    /// Nombre d'abonnés actuels (fenêtre locale, clients distants).
    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn un_evenement_atteint_tous_les_abonnes() {
        // C'est la raison d'être du bus : la fenêtre locale et un client
        // distant doivent voir exactement le même flux.
        let bus = EventBus::new();
        let mut a = bus.subscribe();
        let mut b = bus.subscribe();
        bus.emit("session:token", serde_json::json!({ "token": "salut" }));

        let recu_a = a.recv().await.unwrap();
        let recu_b = b.recv().await.unwrap();
        assert_eq!(recu_a.name, "session:token");
        assert_eq!(recu_a.payload["token"], "salut");
        assert_eq!(recu_b.payload["token"], "salut");
    }

    #[tokio::test]
    async fn emettre_sans_abonne_ne_panique_pas() {
        // Cas courant : traitement en cours alors que la fenêtre est fermée.
        let bus = EventBus::new();
        bus.emit("plan:update", serde_json::json!({ "x": 1 }));
        assert_eq!(bus.receiver_count(), 0);
    }

    #[tokio::test]
    async fn un_abonne_lent_perd_les_plus_anciens_sans_bloquer_les_autres() {
        // Un canal non borné ferait enfler la mémoire ; bloquer l'émission
        // ferait bégayer le flux de jetons pour tout le monde.
        let bus = EventBus::new();
        let mut lent = bus.subscribe();
        for i in 0..(CAPACITE + 10) {
            bus.emit("session:token", serde_json::json!({ "i": i }));
        }
        match lent.recv().await {
            Err(broadcast::error::RecvError::Lagged(perdus)) => {
                assert!(perdus > 0, "le retard doit être signalé");
            }
            autre => panic!("un retard était attendu, obtenu : {autre:?}"),
        }
        // Après le signalement, la réception reprend au plus ancien disponible.
        assert!(lent.recv().await.is_ok());
    }

    #[tokio::test]
    async fn une_charge_non_serialisable_ne_fait_pas_tomber_l_emetteur() {
        #[derive(Debug)]
        struct Impossible;
        impl Serialize for Impossible {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("non sérialisable"))
            }
        }
        let bus = EventBus::new();
        let mut abonne = bus.subscribe();
        bus.emit("mauvais", Impossible);
        bus.emit("bon", serde_json::json!({}));
        // Seul l'événement valide passe.
        assert_eq!(abonne.recv().await.unwrap().name, "bon");
    }
}
