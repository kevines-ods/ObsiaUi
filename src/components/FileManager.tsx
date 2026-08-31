/**
 * Notes du coffre : navigation et édition.
 *
 * L'écriture reste bornée à `brouillon/` par la sandbox du backend — le reste
 * du coffre passe par un patch relu.
 *
 * `openPath` permet à la vue graphique d'amener ici la note sur laquelle on
 * vient de cliquer, sans dupliquer l'éditeur.
 */
import { useCallback, useEffect, useState } from "react";

import * as ipc from "../lib/ipc";
import type { VaultEntry } from "../types/ipc";

export default function FileManager({
  openPath,
}: {
  /** Note à ouvrir, demandée de l'extérieur. */
  openPath?: string | null;
} = {}): React.JSX.Element {
  const [vaultPath, setVaultPath] = useState<string>("");
  const [entries, setEntries] = useState<VaultEntry[]>([]);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [original, setOriginal] = useState("");
  const [loading, setLoading] = useState(true);
  const [loadingNote, setLoadingNote] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [newName, setNewName] = useState("");

  const loadVault = useCallback(async (): Promise<void> => {
    setLoading(true);
    setError(null);
    try {
      const [root, list] = await Promise.all([ipc.vaultPath(), ipc.vaultList()]);
      setVaultPath(root);
      setEntries(list);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadVault();
  }, [loadVault]);

  const openNote = useCallback(async (path: string): Promise<void> => {
    setLoadingNote(true);
    setError(null);
    try {
      const text = await ipc.vaultRead(path);
      setSelectedPath(path);
      setContent(text);
      setOriginal(text);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoadingNote(false);
    }
  }, []);

  // Ouverture demandée par la vue graphique.
  useEffect(() => {
    if (openPath) void openNote(openPath);
  }, [openPath, openNote]);

  const save = useCallback(async (): Promise<void> => {
    if (!selectedPath) return;
    setSaving(true);
    setError(null);
    setNotice(null);
    try {
      const entry = await ipc.vaultWrite(selectedPath, content);
      setOriginal(content);
      setNotice(`Sauvegardé · ${entry.modified}`);
      setEntries((prev) =>
        prev.map((en) => (en.path === entry.path ? entry : en)),
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }, [selectedPath, content]);

  const createNote = useCallback(async (): Promise<void> => {
    const name = newName.trim();
    if (!name) return;
    const path = name.endsWith(".md") ? name : `${name}.md`;
    setNewName("");
    try {
      await ipc.vaultWrite(path, "");
      await loadVault();
      setNotice(`Note « ${path} » créée.`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [newName, loadVault]);

  const isDirty = content !== original;

  return (
    <div className="file-manager">
      <div className="fm-toolbar">
        <span className="fm-title">Coffre</span>
        <span className="fm-path" title={vaultPath}>
          {vaultPath || "…"}
        </span>
        <button type="button" className="btn btn-mini" onClick={() => void loadVault()}>
          ↻
        </button>
      </div>

      {error && (
        <div className="error-banner" role="alert">
          ⚠️ {error}
        </div>
      )}

      <div className="fm-new">
        <input
          type="text"
          placeholder="Nouvelle note…"
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void createNote();
          }}
          aria-label="Nom de la nouvelle note"
        />
        <button type="button" className="btn btn-mini" onClick={() => void createNote()}>
          +
        </button>
      </div>

      <div className="fm-list">
        {loading && <p className="empty-hint">Chargement du coffre…</p>}
        {!loading && entries.length === 0 && (
          <p className="empty-hint">Aucune note trouvée.</p>
        )}
        {entries.map((entry) => (
          <button
            type="button"
            key={entry.path}
            className={entry.path === selectedPath ? "fm-item active" : "fm-item"}
            onClick={() => void openNote(entry.path)}
            title={entry.path}
          >
            <span className="fm-item-name">{entry.name}</span>
            <span className="fm-item-meta">{entry.modified}</span>
          </button>
        ))}
      </div>

      {selectedPath && (
        <div className="fm-editor">
          <div className="fm-editor-head">
            <span className="fm-item-name">{selectedPath}</span>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void save()}
              disabled={!isDirty || saving}
            >
              {saving ? "…" : "Sauvegarder"}
            </button>
          </div>
          {loadingNote ? (
            <p className="empty-hint">Lecture…</p>
          ) : (
            <textarea
              value={content}
              onChange={(e) => setContent(e.target.value)}
              aria-label={`Contenu de ${selectedPath}`}
              spellCheck={false}
            />
          )}
          {notice && <p className="notice">{notice}</p>}
        </div>
      )}
    </div>
  );
}
