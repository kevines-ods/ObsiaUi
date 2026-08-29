/**
 * Contexte applicatif partagé : providers, config et sélection de modèle.
 *
 * Fournit aux 3 zones les données du contrat IPC (providers_list, config_get,
 * provider_test) et gère la sélection fournisseur/modèle ainsi que la
 * persistance du `defaultProvider` via `config_set`.
 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import * as ipc from "../lib/ipc";
import { loadAgents } from "../lib/agents";
import type { AgentInfo } from "../types/ipc";
import type { ConfigPatch, ConfigView, ProviderHealth, ProviderInfo } from "../types/ipc";

interface AppContextValue {
  config: ConfigView | null;
  providers: ProviderInfo[];
  loadingProviders: boolean;
  loadError: string | null;
  selectedProviderId: string;
  selectedModel: string;
  selectProviderAndModel: (providerId: string, model: string) => void;
  refreshProviders: () => Promise<void>;
  refreshConfig: () => Promise<void>;
  saveConfig: (patch: ConfigPatch) => Promise<ConfigView | null>;
  testProvider: (providerId: string) => Promise<ProviderHealth>;
  health: Record<string, ProviderHealth>;
  testingId: string | null;
  // Agents (IA/agents/*.md)
  agents: AgentInfo[];
  loadingAgents: boolean;
  selectedAgent: string | null;
  loadAgents: () => Promise<void>;
  selectAgent: (name: string) => void;
}

const AppContext = createContext<AppContextValue | null>(null);

/** Hook d'accès au contexte applicatif. */
export function useApp(): AppContextValue {
  const ctx = useContext(AppContext);
  if (!ctx) {
    throw new Error("useApp doit être utilisé dans <AppProvider>");
  }
  return ctx;
}

export function AppProvider({ children }: { children: ReactNode }): React.JSX.Element {
  const [config, setConfig] = useState<ConfigView | null>(null);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [loadingProviders, setLoadingProviders] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selectedProviderId, setSelectedProviderId] = useState("");
  const [selectedModel, setSelectedModel] = useState("");
  const [health, setHealth] = useState<Record<string, ProviderHealth>>({});
  const [testingId, setTestingId] = useState<string | null>(null);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [loadingAgents, setLoadingAgents] = useState(true);
  const [selectedAgent, setSelectedAgent] = useState<string | null>(null);
  const hydratedRef = useRef(false);

  const refreshConfig = useCallback(async (): Promise<void> => {
    try {
      const cfg = await ipc.configGet();
      setConfig(cfg);
    } catch (e) {
      setLoadError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const refreshProviders = useCallback(async (): Promise<void> => {
    setLoadingProviders(true);
    setLoadError(null);
    try {
      const list = await ipc.providersList();
      setProviders(list);
      // Sélection initiale / resynchronisation sur le provider par défaut.
      setSelectedProviderId((prev) => {
        if (prev && list.some((p) => p.id === prev)) return prev;
        const preferred = config?.defaultProvider ?? "";
        return (
          (preferred && list.find((p) => p.id === preferred)?.id) ||
          list[0]?.id ||
          ""
        );
      });
    } catch (e) {
      setLoadError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoadingProviders(false);
    }
  }, [config?.defaultProvider]);

  const refreshAgents = useCallback(async (): Promise<void> => {
    setLoadingAgents(true);
    try {
      const list = await loadAgents();
      setAgents(list);
      // Sélection initiale : premier agent par défaut.
      setSelectedAgent((prev) =>
        prev && list.some((a) => a.name === prev) ? prev : (list[0]?.name ?? null),
      );
    } catch (e) {
      setLoadError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoadingAgents(false);
    }
  }, []);

  const selectAgent = useCallback((name: string): void => {
    setSelectedAgent(name);
  }, []);

  // Hydratation initiale : config, providers puis agents.
  useEffect(() => {
    if (hydratedRef.current) return;
    hydratedRef.current = true;
    void (async () => {
      await refreshConfig();
      await refreshProviders();
      await refreshAgents();
    })();
  }, [refreshConfig, refreshProviders, refreshAgents]);

  // Les modèles du provider sélectionné alimentent la sélection par défaut.
  useEffect(() => {
    const provider = providers.find((p) => p.id === selectedProviderId);
    if (!provider) {
      setSelectedModel("");
      return;
    }
    setSelectedModel((prev) =>
      prev && provider.models.some((m) => m.id === prev)
        ? prev
        : (provider.models[0]?.id ?? ""),
    );
  }, [providers, selectedProviderId]);

  const selectProviderAndModel = useCallback(
    (providerId: string, model: string): void => {
      setSelectedProviderId(providerId);
      setSelectedModel(model);
      // Persistance du fournisseur par défaut (best-effort).
      void ipc.configSet({ defaultProvider: providerId }).then(setConfig).catch(() => {
        // silencieux : la config reste valide en mémoire.
      });
    },
    [],
  );

  const saveConfig = useCallback(async (patch: ConfigPatch): Promise<ConfigView | null> => {
    try {
      const cfg = await ipc.configSet(patch);
      setConfig(cfg);
      return cfg;
    } catch (e) {
      setLoadError(e instanceof Error ? e.message : String(e));
      return null;
    }
  }, []);

  const testProvider = useCallback(async (providerId: string): Promise<ProviderHealth> => {
    setTestingId(providerId);
    try {
      const res = await ipc.providerTest(providerId);
      setHealth((prev) => ({ ...prev, [providerId]: res }));
      return res;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      const res: ProviderHealth = { providerId, ok: false, error: msg };
      setHealth((prev) => ({ ...prev, [providerId]: res }));
      return res;
    } finally {
      setTestingId(null);
    }
  }, []);

  const value = useMemo<AppContextValue>(
    () => ({
      config,
      providers,
      loadingProviders,
      loadError,
      selectedProviderId,
      selectedModel,
      selectProviderAndModel,
      refreshProviders,
      refreshConfig,
      saveConfig,
      testProvider,
      health,
      testingId,
      // Agents
      agents,
      loadingAgents,
      selectedAgent,
      loadAgents: refreshAgents,
      selectAgent,
    }),
    [
      config,
      providers,
      loadingProviders,
      loadError,
      selectedProviderId,
      selectedModel,
      selectProviderAndModel,
      refreshProviders,
      refreshConfig,
      saveConfig,
      testProvider,
      health,
      testingId,
      // Agents
      agents,
      loadingAgents,
      selectedAgent,
      refreshAgents,
      selectAgent,
    ],
  );

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}
