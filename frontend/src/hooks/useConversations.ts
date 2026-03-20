import { useCallback, useEffect, useState } from "react";
import { api } from "../api/client";
import type { Conversation } from "../api/types";

export function useConversations(token: string | null) {
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [loading, setLoading] = useState(true);

  const fetchConversations = useCallback(async () => {
    if (!token) {
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const data = await api.listConversations(token);
      setConversations(data);
    } catch {
      // silently ignore — UI will just show empty list
    } finally {
      setLoading(false);
    }
  }, [token]);

  useEffect(() => {
    fetchConversations();
  }, [fetchConversations]);

  const createConversation = useCallback(
    async (name?: string): Promise<Conversation | null> => {
      if (!token) return null;
      try {
        const conv = await api.createConversation(token, name ? { name } : {});
        setConversations((prev) => [conv, ...prev]);
        return conv;
      } catch {
        return null;
      }
    },
    [token],
  );

  const deleteConversation = useCallback(
    async (id: string): Promise<boolean> => {
      if (!token) return false;
      try {
        await api.deleteConversation(token, id);
        setConversations((prev) => prev.filter((c) => c.id !== id));
        return true;
      } catch {
        return false;
      }
    },
    [token],
  );

  const renameConversation = useCallback(
    async (id: string, name: string): Promise<boolean> => {
      if (!token) return false;
      try {
        const updated = await api.renameConversation(token, id, { name });
        setConversations((prev) =>
          prev.map((c) => (c.id === id ? updated : c)),
        );
        return true;
      } catch {
        return false;
      }
    },
    [token],
  );

  return {
    conversations,
    loading,
    refresh: fetchConversations,
    createConversation,
    deleteConversation,
    renameConversation,
  };
}
