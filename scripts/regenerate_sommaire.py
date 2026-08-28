#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Régénère automatiquement les sommaire.md depuis le système de fichiers.
À exécuter AVANT tout commit pour garder un diff Git fiable.

Usage :
    python scripts/regenerate_sommaire.py
"""
import os
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(SCRIPT_DIR, ".."))

def to_rel(p):
    """Convertit un chemin (abs ou relatif) en chemin relatif au coffre."""
    return os.path.relpath(p, ROOT)

def regen(path):
    full = os.path.join(ROOT, to_rel(path))
    parent = os.path.dirname(full)
    if not os.path.isfile(full) or not to_rel(path).endswith("sommaire.md"):
        return
    base = os.path.basename(to_rel(path))
    titre = base.split("—", 1)[1].strip() if "—" in base else "Sommaire"
    sous = [d for d in os.listdir(parent)
            if os.path.isdir(os.path.join(parent, d))]
    rows = []
    for d in sous:
        rows.append("[%s](%s/%s/)| (sous-dossier)" % (d, to_rel(parent), d))
    out = "# " + titre + "\n\n"
    out += "> Généré automatiquement. Ne pas éditer à la main.\n\n"
    out += "## Sous-dossiers\n\n"
    out += "| Dossier | Contenu |\n|---|---|\n"
    out += "\n".join(["| " + r + " |" for r in rows]) + "\n"
    with open(full, "w", encoding="utf-8") as f:
        f.write(out)

print("Régénération des sommaires…")
for dirpath, dirs, files in os.walk(ROOT):
    for fl in files:
        if fl == "sommaire.md":
            regen(os.path.join(dirpath, fl))
print("Fait.")
