// Exemple minimal de plugin ObsiaUi.
//
// Le fichier est évalué avec un seul argument, `obsia`, dont la surface est
// taillée par les permissions du manifeste : ici `sessions`, qui ouvre
// `sessions_list`. Demander une commande non accordée renvoie une erreur.

obsia.onMount("status-bar", (element) => {
  const rendre = async () => {
    try {
      const sessions = await obsia.call("sessions_list");
      element.textContent = `${sessions.length} session(s)`;
    } catch (e) {
      element.textContent = `compteur : ${e.message}`;
    }
  };

  void rendre();

  // Le flux d'événements est le même que celui de l'interface.
  const abonnement = obsia.subscribe("session:done", rendre);

  // Le nettoyage rendu est appelé quand la zone disparaît.
  return () => {
    void abonnement.then((desabonner) => desabonner());
  };
});
