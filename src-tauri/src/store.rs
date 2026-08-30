//! Stockage JSON sur disque, un fichier par entité.
//!
//! Partagé par les sessions, les équipes et les plans. Trois garanties, qui
//! sont les raisons d'avoir ce module plutôt que des `fs::write` éparpillés :
//!
//! - **Écriture atomique.** On écrit un fichier temporaire puis on le
//!   `rename` : sur le même système de fichiers, le remplacement est
//!   atomique. Un lecteur voit l'ancienne version ou la nouvelle, jamais un
//!   JSON tronqué par une coupure.
//! - **Identifiants validés.** Un identifiant venu de l'IPC sert de nom de
//!   fichier ; sans barrière, `../../.ssh/config` sortirait du dossier.
//! - **Tolérance à la corruption.** Une entité illisible est ignorée à la
//!   lecture d'ensemble, avec un avertissement : un fichier abîmé ne doit pas
//!   rendre la liste entière inutilisable.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tracing::warn;

/// Longueur maximale d'un identifiant (un UUID en fait 36).
const ID_MAX: usize = 64;

/// Un identifiant valide : minuscules ASCII, chiffres et tirets.
///
/// Volontairement plus strict que nécessaire — les identifiants sont des UUID
/// engendrés en interne, et rien ne justifie d'accepter autre chose.
pub fn est_id_valide(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= ID_MAX
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub struct JsonStore {
    dir: PathBuf,
}

impl JsonStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path_for(&self, id: &str) -> Result<PathBuf, String> {
        if !est_id_valide(id) {
            return Err(format!("identifiant invalide : {id}"));
        }
        Ok(self.dir.join(format!("{id}.json")))
    }

    /// Écrit une entité de façon atomique.
    pub fn save<T: Serialize>(&self, id: &str, valeur: &T) -> Result<(), String> {
        let path = self.path_for(id)?;
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("création de {} impossible: {e}", self.dir.display()))?;
        let raw =
            serde_json::to_string_pretty(valeur).map_err(|e| format!("sérialisation: {e}"))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, raw).map_err(|e| format!("écriture de {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            // Le temporaire ne doit pas rester derrière un échec de rename.
            let _ = std::fs::remove_file(&tmp);
            format!("remplacement de {}: {e}", path.display())
        })
    }

    pub fn load<T: DeserializeOwned>(&self, id: &str) -> Result<T, String> {
        let path = self.path_for(id)?;
        let raw = std::fs::read_to_string(&path).map_err(|e| format!("{id} illisible: {e}"))?;
        serde_json::from_str(&raw).map_err(|e| format!("{id} corrompu: {e}"))
    }

    /// Suppression idempotente : supprimer deux fois n'est pas une erreur.
    pub fn delete(&self, id: &str) -> Result<(), String> {
        let path = self.path_for(id)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("suppression de {}: {e}", path.display())),
        }
    }

    /// Toutes les entités lisibles du dossier. Les autres sont ignorées.
    pub fn load_all<T: DeserializeOwned>(&self) -> Vec<T> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .filter_map(|e| {
                let chemin = e.path();
                match std::fs::read_to_string(&chemin) {
                    Ok(raw) => match serde_json::from_str::<T>(&raw) {
                        Ok(v) => Some(v),
                        Err(err) => {
                            warn!(fichier = %chemin.display(), %err, "entité corrompue, ignorée");
                            None
                        }
                    },
                    Err(err) => {
                        warn!(fichier = %chemin.display(), %err, "entité illisible, ignorée");
                        None
                    }
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Exemple {
        id: String,
        valeur: u32,
    }

    fn store() -> (tempfile::TempDir, JsonStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonStore::new(dir.path().join("entites"));
        (dir, store)
    }

    #[test]
    fn aller_retour() {
        let (_d, s) = store();
        let e = Exemple {
            id: "abc-1".into(),
            valeur: 42,
        };
        s.save(&e.id, &e).unwrap();
        assert_eq!(s.load::<Exemple>("abc-1").unwrap(), e);
    }

    #[test]
    fn aucun_temporaire_ne_subsiste() {
        let (_d, s) = store();
        s.save(
            "abc-1",
            &Exemple {
                id: "abc-1".into(),
                valeur: 1,
            },
        )
        .unwrap();
        let restes: Vec<_> = std::fs::read_dir(s.dir())
            .unwrap()
            .flatten()
            .filter(|e| e.path().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(restes.is_empty());
    }

    #[test]
    fn un_identifiant_hostile_est_refuse() {
        let (_d, s) = store();
        for mauvais in ["../evasion", "/etc/passwd", "a/b", "", "MAJ", "a b", "a.b"] {
            assert!(
                s.path_for(mauvais).is_err(),
                "l'identifiant {mauvais:?} aurait dû être refusé"
            );
        }
        assert!(est_id_valide("3f2a1b8c-0000-4000-8000-000000000000"));
        assert!(!est_id_valide(&"a".repeat(ID_MAX + 1)));
    }

    #[test]
    fn une_entite_corrompue_n_empeche_pas_de_lire_les_autres() {
        let (_d, s) = store();
        s.save(
            "bonne",
            &Exemple {
                id: "bonne".into(),
                valeur: 7,
            },
        )
        .unwrap();
        std::fs::write(s.dir().join("cassee.json"), b"{ pas du json").unwrap();
        let toutes: Vec<Exemple> = s.load_all();
        assert_eq!(toutes.len(), 1);
        assert_eq!(toutes[0].valeur, 7);
    }

    #[test]
    fn lire_un_dossier_absent_ne_panique_pas() {
        let s = JsonStore::new("/inexistant/vraiment");
        assert!(s.load_all::<Exemple>().is_empty());
    }

    #[test]
    fn supprimer_deux_fois_reste_un_succes() {
        let (_d, s) = store();
        s.save(
            "x",
            &Exemple {
                id: "x".into(),
                valeur: 1,
            },
        )
        .unwrap();
        s.delete("x").unwrap();
        s.delete("x").unwrap();
        assert!(s.load::<Exemple>("x").is_err());
    }

    #[test]
    fn les_fichiers_non_json_sont_ignores() {
        let (_d, s) = store();
        std::fs::create_dir_all(s.dir()).unwrap();
        std::fs::write(s.dir().join("notes.txt"), b"bonjour").unwrap();
        assert!(s.load_all::<Exemple>().is_empty());
    }
}
