//! L'intendant : l'agent intégré qui configure l'application par le chat.
//!
//! Il ne vient pas du coffre. Les agents du coffre décrivent un travail ;
//! l'intendant, lui, agit sur ObsiaUi — thème, sessions, équipes,
//! planifications, déclarations MCP, serveur distant. Il porte un nom distinct
//! de l'agent `assistant` du coffre pour qu'on ne les confonde jamais.
//!
//! # Comment il agit
//!
//! Il répond en langage naturel et joint un bloc JSON décrivant les actions.
//! Pas d'appel d'outils : celui-ci n'existe que sur une partie des modèles, et
//! exclurait les petits modèles locaux qui font tout l'intérêt d'un harness
//! qui tourne chez soi. Le même mécanisme fait déjà ses preuves pour la
//! décomposition des planifications.
//!
//! # Ce qu'il ne peut pas faire
//!
//! Le catalogue est une **liste blanche**. En sont volontairement absents :
//!
//! - **l'activation d'un plugin** — elle vaut approbation d'un code qui
//!   s'exécutera dans la fenêtre ; cela reste une décision humaine, prise
//!   après lecture ;
//! - **les clés d'API et le jeton distant** — un secret ne se manipule pas
//!   par une phrase mal comprise ;
//! - **l'écriture dans le coffre** hors `brouillon/`, que la sandbox refuse
//!   de toute façon. La seule action qui écrit, `mcp`, vise `brouillon/` :
//!   un outil MCP donne des accès, et le contrat veut qu'ils soient relus
//!   avant d'entrer dans le coffre.
//!
//! # Aperçu avant exécution
//!
//! Rien n'est appliqué à la lecture de la réponse. Les actions sont décrites
//! en clair et attendent une validation. Un modèle qui se trompe de thème est
//! sans conséquence ; un modèle qui supprime une planification par
//! contresens, beaucoup moins.

use crate::plan::PlanStep;
use crate::team::{TeamMember, TeamStrategy};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Nom de l'agent intégré.
pub const NOM: &str = "intendant";

/// Nombre maximal d'actions par réponse.
///
/// Une réponse qui en propose trente est un emballement, pas une intention :
/// mieux vaut la refuser que faire relire une liste que personne ne lira.
pub const MAX_ACTIONS: usize = 12;

/// Une action que l'intendant peut proposer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum Action {
    /// Thème de l'interface.
    Theme { theme: String },
    /// Fournisseur retenu par défaut.
    FournisseurDefaut { provider_id: String },
    /// Ouvre une session de conversation.
    Session {
        #[serde(default)]
        agent: Option<String>,
        #[serde(default)]
        provider: Option<String>,
        model: String,
    },
    /// Compose une équipe d'agents.
    Equipe {
        name: String,
        #[serde(default)]
        description: String,
        members: Vec<TeamMember>,
        strategy: TeamStrategy,
        max_turns: u32,
    },
    /// Crée une planification.
    Planification {
        title: String,
        objective: String,
        steps: Vec<PlanStep>,
    },
    /// Crée un patch d'interface (thème et disposition).
    Patch {
        name: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        theme: BTreeMap<String, String>,
    },
    /// Applique ou retire un patch existant.
    PatchActif { patch_id: String, enabled: bool },
    /// Rédige une déclaration d'outil MCP dans `brouillon/`.
    ///
    /// Jamais dans `IA/MCP/` : le coffre est en lecture seule hors
    /// `brouillon/`, et donner à des agents un accès que personne n'a relu
    /// serait exactement le geste que le contrat interdit. L'humain déplace
    /// la note après lecture.
    Mcp {
        name: String,
        #[serde(default)]
        description: String,
        /// Corps de la note : à quoi sert l'outil, comment le brancher.
        #[serde(default)]
        body: String,
    },
    /// Démarre ou arrête le serveur distant.
    Distant { enabled: bool },
}

impl Action {
    /// Description en clair, affichée avant validation.
    ///
    /// C'est ce que l'utilisateur lit pour décider : elle doit nommer l'effet,
    /// pas répéter le JSON.
    pub fn describe(&self) -> String {
        match self {
            Action::Theme { theme } => format!("Passer l'interface en thème « {theme} »"),
            Action::FournisseurDefaut { provider_id } => {
                format!("Choisir « {provider_id} » comme fournisseur par défaut")
            }
            Action::Session { agent, model, .. } => match agent {
                Some(a) => format!("Ouvrir une session avec l'agent « {a} » sur {model}"),
                None => format!("Ouvrir une session sur {model}"),
            },
            Action::Equipe {
                name,
                members,
                strategy,
                max_turns,
                ..
            } => format!(
                "Composer l'équipe « {name} » — {} ({}), {max_turns} tours max",
                members
                    .iter()
                    .map(|m| m.agent.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                match strategy {
                    TeamStrategy::Supervisor => "superviseur",
                    TeamStrategy::RoundRobin => "tour de rôle",
                }
            ),
            Action::Planification { title, steps, .. } => {
                format!(
                    "Créer la planification « {title} » — {} étape(s)",
                    steps.len()
                )
            }
            Action::Patch { name, theme, .. } => {
                format!(
                    "Créer le patch d'interface « {name} » — {} jeton(s)",
                    theme.len()
                )
            }
            Action::PatchActif { patch_id, enabled } => {
                if *enabled {
                    format!("Appliquer le patch {patch_id}")
                } else {
                    format!("Retirer le patch {patch_id}")
                }
            }
            Action::Mcp { name, .. } => {
                // Le chemin exact, pas le dossier : le nom est translittéré en
                // slug, et on ne retrouve pas « Chrome DevTools » en cherchant
                // ce nom-là dans brouillon/.
                format!(
                    "Rédiger la déclaration MCP « {name} » dans \
                     brouillon/IA/MCP/{}.md — à relire avant de l'entrer dans \
                     le coffre",
                    crate::session::slug(name)
                )
            }
            Action::Distant { enabled } => {
                if *enabled {
                    "Démarrer le serveur distant".to_string()
                } else {
                    "Arrêter le serveur distant".to_string()
                }
            }
        }
    }

    /// Vérifications qui ne dépendent que de l'action elle-même.
    ///
    /// Les contrôles de fond (équipe cohérente, plan sans cycle, valeur CSS
    /// sûre) restent dans leurs modules : les refaire ici les ferait diverger.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Action::Theme { theme } => {
                if matches!(theme.as_str(), "dark" | "light" | "system") {
                    Ok(())
                } else {
                    Err(format!(
                        "thème inconnu : {theme} (attendu dark, light ou system)"
                    ))
                }
            }
            Action::FournisseurDefaut { provider_id } => {
                if provider_id.trim().is_empty() {
                    Err("fournisseur requis".into())
                } else {
                    Ok(())
                }
            }
            Action::Session { model, .. } => {
                if model.trim().is_empty() {
                    Err("modèle requis pour ouvrir une session".into())
                } else {
                    Ok(())
                }
            }
            Action::Equipe { name, members, .. } => {
                if name.trim().is_empty() {
                    return Err("nom d'équipe requis".into());
                }
                if members.is_empty() {
                    return Err("une équipe compte au moins un membre".into());
                }
                Ok(())
            }
            Action::Planification { title, steps, .. } => {
                if title.trim().is_empty() {
                    return Err("titre de planification requis".into());
                }
                if steps.is_empty() {
                    return Err("une planification compte au moins une étape".into());
                }
                Ok(())
            }
            Action::Patch { name, .. } => {
                if name.trim().is_empty() {
                    Err("nom de patch requis".into())
                } else {
                    Ok(())
                }
            }
            Action::PatchActif { patch_id, .. } => {
                if patch_id.trim().is_empty() {
                    Err("identifiant de patch requis".into())
                } else {
                    Ok(())
                }
            }
            Action::Mcp { name, .. } => {
                if name.trim().is_empty() {
                    Err("nom d'outil MCP requis".into())
                } else {
                    Ok(())
                }
            }
            Action::Distant { .. } => Ok(()),
        }
    }
}

/// Enveloppe attendue dans la réponse du modèle.
#[derive(Debug, Deserialize)]
struct Enveloppe {
    #[serde(default)]
    actions: Vec<Action>,
}

/// Actions proposées, avec leur description, telles qu'on les soumet à la
/// validation humaine.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposition {
    pub actions: Vec<Action>,
    pub descriptions: Vec<String>,
}

/// Extrait les actions d'une réponse de modèle.
///
/// Une réponse sans bloc JSON n'est **pas** une erreur : l'intendant répond
/// souvent à une question sans rien avoir à changer.
pub fn extraire_actions(reponse: &str) -> Result<Option<Proposition>, String> {
    let Some(json) = crate::plan::extraire_json(reponse) else {
        return Ok(None);
    };
    let enveloppe: Enveloppe = match serde_json::from_str(json) {
        Ok(e) => e,
        // Un JSON présent mais illisible ne doit pas faire échouer le tour :
        // c'est souvent un exemple cité dans la réponse.
        Err(_) => return Ok(None),
    };
    if enveloppe.actions.is_empty() {
        return Ok(None);
    }
    if enveloppe.actions.len() > MAX_ACTIONS {
        return Err(format!(
            "{} actions proposées, au-delà de la limite de {MAX_ACTIONS} — \
             reformulez en demandant moins de choses à la fois",
            enveloppe.actions.len()
        ));
    }
    for a in &enveloppe.actions {
        a.validate()?;
    }
    let descriptions = enveloppe.actions.iter().map(Action::describe).collect();
    Ok(Some(Proposition {
        actions: enveloppe.actions,
        descriptions,
    }))
}

/// Prompt système de l'intendant.
///
/// La liste des agents, des fournisseurs et des outils MCP y est injectée :
/// sans elle, le modèle invente des noms plausibles et propose des actions qui
/// échouent à la validation. Les MCP servent surtout à ne pas redéclarer un
/// outil que le coffre possède déjà.
pub fn prompt(agents: &[String], fournisseurs: &[(String, Vec<String>)], mcp: &[String]) -> String {
    let mut modeles = Vec::new();
    for (fournisseur, liste) in fournisseurs {
        for m in liste.iter().take(6) {
            modeles.push(format!("{fournisseur}/{m}"));
        }
    }

    format!(
        r##"# Intendant

Tu es l'intendant d'ObsiaUi, l'interface qui pilote le coffre OBSIA. Tu
configures **l'application** : thème, sessions, équipes, planifications,
serveur distant. Tu n'es pas un agent du coffre et tu ne rédiges pas de notes.

## Ce que tu sais de l'installation

Agents disponibles : {agents}
Modèles disponibles (fournisseur/modèle) : {modeles}
Outils MCP déjà déclarés dans le coffre : {mcp}

## Comment tu réponds

Réponds d'abord normalement, en français, en expliquant ce que tu proposes.

Si — et seulement si — la demande implique de changer quelque chose, ajoute à
la fin un bloc JSON :

```json
{{"actions": [ {{"action": "theme", "theme": "light"}} ]}}
```

Une question qui n'appelle aucun changement se répond sans bloc JSON.

## Actions disponibles

- `theme` — `theme` vaut `dark`, `light` ou `system`.
- `fournisseur-defaut` — `providerId`.
- `session` — `model` obligatoire ; `agent` et `provider` facultatifs.
- `equipe` — `name`, `members` (chacun `agent`, `model`, `provider`, `role`),
  `strategy` (`round-robin` ou `supervisor`), `maxTurns`.
- `planification` — `title`, `objective`, `steps` (chacune `id`, `title`,
  `instruction`, `agent`, `model`, `dependsOn`).
- `patch` — `name` et `theme`, un dictionnaire de jetons CSS
  (`{{"panel-bg": "#101216"}}`). Valeurs simples uniquement : ni `url(`, ni
  `@import`, ni point-virgule.
- `patch-actif` — `patchId` et `enabled`.
- `mcp` — `name`, `description`, `body`. Rédige la déclaration d'un outil MCP.
  Elle part dans `brouillon/IA/MCP/`, **jamais** dans `IA/MCP/` : le coffre est
  en lecture seule hors brouillon, et un outil donne des accès qui doivent
  être relus avant d'entrer. Dis-le dans ta réponse plutôt que de laisser
  croire que l'outil est déjà branché. Dans `body`, mets ce qu'il faut pour le
  brancher : à quoi il sert, la commande ou l'URL du serveur, ce qu'il expose.
- `distant` — `enabled`.

## Règles

- N'emploie que des agents et des modèles de la liste ci-dessus. N'en invente
  aucun : une action qui nomme un agent inexistant sera refusée.
- Au plus {MAX_ACTIONS} actions par réponse. Si la demande en exige plus,
  propose les plus utiles et dis ce que tu as laissé de côté.
- Tes actions sont **soumises à validation** avant d'être appliquées. Décris
  donc clairement leur effet dans ta réponse.
- Tu ne peux ni activer un plugin, ni lire ou changer une clé d'API, ni
  toucher au jeton du serveur distant. Si on te le demande, explique que ces
  gestes passent par les réglages, volontairement.
- Tu n'écris nulle part dans le coffre sauf `brouillon/`, et seule l'action
  `mcp` y écrit. Un outil MCP déjà présent dans la liste ci-dessus se
  redéclare rarement : demande plutôt s'il faut le modifier.
- Ne mets jamais de clé, de jeton ni de mot de passe dans une déclaration MCP.
  Décris la variable d'environnement à définir, pas sa valeur.
"##,
        agents = if agents.is_empty() {
            "(aucun — le coffre est introuvable)".to_string()
        } else {
            agents.join(", ")
        },
        modeles = if modeles.is_empty() {
            "(aucun — lance la détection des moteurs locaux)".to_string()
        } else {
            modeles.join(", ")
        },
        mcp = if mcp.is_empty() {
            "(aucun)".to_string()
        } else {
            mcp.join(", ")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bloc(actions: &str) -> String {
        format!("Voici ce que je propose.\n\n```json\n{{\"actions\": {actions}}}\n```\n")
    }

    // ===== Extraction =====

    #[test]
    fn une_reponse_sans_bloc_ne_propose_rien() {
        // L'intendant répond souvent à une question sans rien avoir à changer.
        assert_eq!(
            extraire_actions("Le thème actuel est sombre.").unwrap(),
            None
        );
    }

    #[test]
    fn extrait_une_action_simple() {
        let p = extraire_actions(&bloc(r#"[{"action":"theme","theme":"light"}]"#))
            .unwrap()
            .expect("une proposition attendue");
        assert_eq!(
            p.actions,
            vec![Action::Theme {
                theme: "light".into()
            }]
        );
        assert!(p.descriptions[0].contains("light"));
    }

    #[test]
    fn un_json_illisible_ne_fait_pas_echouer_le_tour() {
        // C'est souvent un exemple cité dans la réponse, pas une intention.
        assert_eq!(
            extraire_actions("exemple : {\"actions\": [oups}").unwrap(),
            None
        );
    }

    #[test]
    fn une_liste_vide_ne_propose_rien() {
        assert_eq!(extraire_actions(&bloc("[]")).unwrap(), None);
    }

    #[test]
    fn une_avalanche_d_actions_est_refusee() {
        // Une réponse qui en propose trente est un emballement : mieux vaut
        // la refuser que faire relire une liste que personne ne lira.
        let une = r#"{"action":"distant","enabled":true}"#;
        let trop = format!("[{}]", vec![une; MAX_ACTIONS + 1].join(","));
        let err = extraire_actions(&bloc(&trop)).unwrap_err();
        assert!(err.contains("limite"));
    }

    #[test]
    fn une_action_invalide_est_signalee_pas_appliquee() {
        let err = extraire_actions(&bloc(r#"[{"action":"theme","theme":"fluo"}]"#)).unwrap_err();
        assert!(err.contains("thème inconnu"), "message : {err}");
    }

    #[test]
    fn une_action_inconnue_est_ignoree_avec_le_reste() {
        // serde refuse l'enveloppe entière : on ne veut pas appliquer une
        // moitié d'intention.
        assert_eq!(
            extraire_actions(&bloc(r#"[{"action":"formater-le-disque"}]"#)).unwrap(),
            None
        );
    }

    // ===== Validation =====

    #[test]
    fn une_session_sans_modele_est_refusee() {
        let a = Action::Session {
            agent: None,
            provider: None,
            model: "  ".into(),
        };
        assert!(a.validate().is_err());
    }

    #[test]
    fn une_equipe_sans_membre_est_refusee() {
        let a = Action::Equipe {
            name: "Revue".into(),
            description: String::new(),
            members: vec![],
            strategy: TeamStrategy::RoundRobin,
            max_turns: 4,
        };
        assert!(a.validate().unwrap_err().contains("membre"));
    }

    #[test]
    fn une_planification_sans_etape_est_refusee() {
        let a = Action::Planification {
            title: "Refonte".into(),
            objective: "obj".into(),
            steps: vec![],
        };
        assert!(a.validate().is_err());
    }

    // ===== Descriptions =====

    #[test]
    fn les_descriptions_nomment_l_effet_pas_le_json() {
        let a = Action::Distant { enabled: false };
        assert_eq!(a.describe(), "Arrêter le serveur distant");
        let b = Action::PatchActif {
            patch_id: "p1".into(),
            enabled: true,
        };
        assert!(b.describe().starts_with("Appliquer"));
    }

    #[test]
    fn la_description_d_une_equipe_nomme_ses_membres() {
        let a = Action::Equipe {
            name: "Revue".into(),
            description: String::new(),
            members: vec![
                TeamMember {
                    agent: "assistant".into(),
                    provider: None,
                    model: "m".into(),
                    role: None,
                },
                TeamMember {
                    agent: "relecteur".into(),
                    provider: None,
                    model: "m".into(),
                    role: None,
                },
            ],
            strategy: TeamStrategy::Supervisor,
            max_turns: 6,
        };
        let d = a.describe();
        assert!(d.contains("assistant, relecteur"));
        assert!(d.contains("superviseur"));
        assert!(d.contains("6 tours"));
    }

    // ===== Action MCP =====

    #[test]
    fn extrait_une_declaration_mcp() {
        let p = extraire_actions(&bloc(
            r#"[{"action":"mcp","name":"chrome-devtools","description":"Pilote un navigateur.","body":"npx @modelcontextprotocol/server-chrome"}]"#,
        ))
        .unwrap()
        .expect("une proposition attendue");
        assert_eq!(
            p.actions,
            vec![Action::Mcp {
                name: "chrome-devtools".into(),
                description: "Pilote un navigateur.".into(),
                body: "npx @modelcontextprotocol/server-chrome".into(),
            }]
        );
    }

    #[test]
    fn la_description_mcp_dit_que_ca_part_en_brouillon() {
        // Sans cela on croit l'outil branché alors qu'il attend une relecture.
        let d = Action::Mcp {
            name: "git-hub".into(),
            description: String::new(),
            body: String::new(),
        }
        .describe();
        assert!(d.contains("git-hub"));
        assert!(d.contains("brouillon/IA/MCP/git-hub.md"));
    }

    #[test]
    fn la_description_mcp_donne_le_chemin_translittere() {
        // « Chrome DevTools » ne se retrouve pas sous ce nom dans brouillon/ :
        // c'est le slug qui sert de nom de fichier.
        let d = Action::Mcp {
            name: "Chrome DevTools".into(),
            description: String::new(),
            body: String::new(),
        }
        .describe();
        assert!(d.contains("brouillon/IA/MCP/chrome-devtools.md"), "{d}");
    }

    #[test]
    fn une_declaration_mcp_sans_nom_est_refusee() {
        assert!(Action::Mcp {
            name: "   ".into(),
            description: String::new(),
            body: String::new(),
        }
        .validate()
        .is_err());
    }

    #[test]
    fn le_corps_et_la_description_mcp_sont_facultatifs() {
        // Un modèle qui ne renseigne que le nom ne doit pas faire échouer le
        // tour entier : la note part avec un corps vide, à compléter.
        let p = extraire_actions(&bloc(r#"[{"action":"mcp","name":"outil"}]"#))
            .unwrap()
            .expect("une proposition attendue");
        assert_eq!(
            p.actions,
            vec![Action::Mcp {
                name: "outil".into(),
                description: String::new(),
                body: String::new(),
            }]
        );
    }

    // ===== Prompt =====

    #[test]
    fn le_prompt_liste_ce_qui_existe_vraiment() {
        // Sans cette liste, le modèle invente des noms plausibles et propose
        // des actions qui échouent à la validation.
        let p = prompt(
            &["assistant".into(), "relecteur".into()],
            &[("ollama".into(), vec!["qwen3:8b".into()])],
            &["git-hub".into()],
        );
        assert!(p.contains("assistant, relecteur"));
        assert!(p.contains("ollama/qwen3:8b"));
        // Sans la liste des MCP existants, il en redéclare un déjà présent.
        assert!(p.contains("git-hub"));
    }

    #[test]
    fn le_prompt_dit_ce_qui_lui_est_interdit() {
        let p = prompt(&[], &[], &[]);
        assert!(p.contains("plugin"));
        assert!(p.contains("clé d'API") || p.contains("clé d’API"));
    }

    #[test]
    fn le_prompt_reste_exploitable_sans_coffre_ni_moteur() {
        let p = prompt(&[], &[], &[]);
        assert!(p.contains("aucun"));
    }
}
