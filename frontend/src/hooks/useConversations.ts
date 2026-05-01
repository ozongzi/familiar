import { useCallback, useEffect, useState } from "react";
import { api } from "../api/client";
import type { Conversation, Folder } from "../api/types";
import { toast } from "../store/toast";

export function useConversations(token: string | null) {
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
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

  const fetchFolders = useCallback(async () => {
    if (!token) return;
    try {
      const data = await api.listFolders(token);
      setFolders(data);
    } catch {
      // silently ignore
    }
  }, [token]);

  useEffect(() => {
    fetchConversations();
    fetchFolders();
  }, [fetchConversations, fetchFolders]);

  const createConversation = useCallback(
    async (
      name?: string,
      modelId?: string | null,
    ): Promise<Conversation | null> => {
      if (!token) return null;
      try {
        const conv = await api.createConversation(token, {
          ...(name ? { name } : {}),
          ...(modelId ? { model_id: modelId } : {}),
        });
        setConversations((prev) => [conv, ...prev]);
        return conv;
      } catch (e) {
        toast.error((e as Error).message ?? "创建对话失败");
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
      } catch (e) {
        toast.error((e as Error).message ?? "删除失败");
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

  const createFolder = useCallback(
    async (name: string, parentId?: string | null): Promise<Folder | null> => {
      if (!token) return null;
      try {
        const folder = await api.createFolder(token, {
          name,
          ...(parentId ? { parent_id: parentId } : {}),
        });
        setFolders((prev) => [...prev, folder]);
        return folder;
      } catch (e) {
        toast.error((e as Error).message ?? "创建文件夹失败");
        return null;
      }
    },
    [token],
  );

  const deleteFolder = useCallback(
    async (id: string): Promise<boolean> => {
      if (!token) return false;
      try {
        await api.deleteFolder(token, id);
        setFolders((prev) => prev.filter((f) => f.id !== id));
        // Also update conversations that were in this folder
        setConversations((prev) =>
          prev.map((c) => (c.folder_id === id ? { ...c, folder_id: null } : c)),
        );
        return true;
      } catch (e) {
        toast.error((e as Error).message ?? "删除文件夹失败");
        return false;
      }
    },
    [token],
  );

  const renameFolder = useCallback(
    async (id: string, name: string): Promise<boolean> => {
      if (!token) return false;
      try {
        const updated = await api.updateFolder(token, id, { name });
        setFolders((prev) => prev.map((f) => (f.id === id ? updated : f)));
        return true;
      } catch {
        return false;
      }
    },
    [token],
  );

  const moveConversation = useCallback(
    async (convId: string, folderId: string | null): Promise<boolean> => {
      if (!token) return false;
      try {
        await api.moveConversation(token, convId, { folder_id: folderId });
        setConversations((prev) =>
          prev.map((c) =>
            c.id === convId ? { ...c, folder_id: folderId } : c,
          ),
        );
        return true;
      } catch (e) {
        toast.error((e as Error).message ?? "移动失败");
        return false;
      }
    },
    [token],
  );

  return {
    conversations,
    folders,
    loading,
    refresh: fetchConversations,
    createConversation,
    deleteConversation,
    renameConversation,
    createFolder,
    deleteFolder,
    renameFolder,
    moveConversation,
  };
}
