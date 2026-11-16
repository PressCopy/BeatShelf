import { useTranslation } from "react-i18next";
import { FilePlus, FolderInput, FolderPlus } from "lucide-react";
import { api, errorText } from "../api";
import { useStore } from "../store";
import { Button, IconButton } from "./ui";

const KNOWN_ERRORS = ["already_in_library", "empty_folder", "too_many_items"];

/**
 * Header buttons of a library section.
 *
 * `create`: "New folder" makes an empty item of the section's kind and opens
 * its card; files are added to it afterwards, and the first one names it.
 * Without `create` (Sorting): pick a folder or files on disk to bring in.
 */
export function AddButtons({ create, mode }: { create?: boolean; mode?: "new" | "inbox" }) {
  const { t } = useTranslation();
  const kind = useStore((s) => s.libraryKind);
  const openAddFiles = useStore((s) => s.openAddFiles);
  const selectBeat = useStore((s) => s.selectBeat);
  const openDetail = useStore((s) => s.openDetail);
  const notify = useStore((s) => s.notify);

  const newFolder = async () => {
    try {
      const row = await api.createItem(kind === "all" ? "beat" : kind, t("draft.name"));
      selectBeat(row.id);
      openDetail(row.id);
    } catch (e) {
      notify(errorText(e), "error");
    }
  };

  const fromDisk = async (folder: boolean) => {
    try {
      const picked = await api.pickImport(folder, t(folder ? "add.pickFolderTitle" : "add.pickFilesTitle"));
      if (!picked) return;
      const { drop, ...suggest } = picked;
      openAddFiles(drop, null, { suggest, mode });
    } catch (e) {
      const m = errorText(e);
      notify(KNOWN_ERRORS.includes(m) ? t(`add.errors.${m}`) : m, "error");
    }
  };

  if (create) {
    return (
      <div className="flex shrink-0 items-center gap-1">
        <Button tone="primary" onClick={() => void newFolder()}>
          <FolderPlus size={14} />
          {t("draft.create")}
        </Button>
        <IconButton label={t("draft.importFolder")} onClick={() => void fromDisk(true)}>
          <FolderInput size={16} />
        </IconButton>
      </div>
    );
  }
  return (
    <div className="flex shrink-0 items-center gap-1">
      <Button tone="primary" onClick={() => void fromDisk(true)}>
        <FolderPlus size={14} />
        {t("add.folder")}
      </Button>
      <IconButton label={t("add.files")} onClick={() => void fromDisk(false)}>
        <FilePlus size={16} />
      </IconButton>
    </div>
  );
}
