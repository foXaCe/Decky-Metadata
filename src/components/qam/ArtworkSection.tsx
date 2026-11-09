import { ButtonItem, Field, PanelSectionRow, TextField } from "@decky/ui";
import { FaImages } from "react-icons/fa";
import { useEffect, useState } from "react";

import { t } from "../../i18n";
import { getArtworkSettings, setArtworkSettings } from "../../backend";
import { applyArtworkToMissing } from "../../artwork";
import { compactTextStyle, inlineStatusStyle } from "../../styles";
import * as log from "../../log";
import { CollapsibleSection } from "./CollapsibleSection";

type ArtworkSectionProps = {
  games: Array<{ appid: number; name: string }>;
};

export function ArtworkSection({ games }: ArtworkSectionProps) {
  const [configured, setConfigured] = useState(false);
  const [batch, setBatch] = useState<{ done: number; total: number } | null>(null);
  const [applied, setApplied] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    void getArtworkSettings()
      .then((settings) => {
        if (!cancelled) setConfigured(settings.configured);
      })
      .catch((cause) => log.warn("artwork", "settings load failed", cause));
    return () => {
      cancelled = true;
    };
  }, []);

  const runBatch = async () => {
    if (batch || !configured) return;
    setApplied(null);
    setBatch({ done: 0, total: games.length });
    try {
      const count = await applyArtworkToMissing(
        games.map((game) => ({ appId: game.appid, title: game.name })),
        (done, total) => setBatch({ done, total }),
      );
      setApplied(count);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBatch(null);
    }
  };

  const save = async () => {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      const saved = await setArtworkSettings(draft.trim());
      setConfigured(saved.configured);
      setDraft("");
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <CollapsibleSection title={t("qam.artwork.title")} icon={<FaImages size={16} />}>
      <PanelSectionRow>
        <div style={compactTextStyle}>
          {configured ? t("qam.artwork.configured") : t("qam.artwork.hint")}
        </div>
      </PanelSectionRow>
      <PanelSectionRow>
        <TextField
          label={t("qam.artwork.keyLabel")}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
        />
      </PanelSectionRow>
      <PanelSectionRow>
        <ButtonItem
          layout="below"
          bottomSeparator="none"
          disabled={busy || draft.trim().length === 0}
          onClick={() => void save()}
        >
          {busy ? t("common.loading") : t("qam.artwork.save")}
        </ButtonItem>
      </PanelSectionRow>
      <PanelSectionRow>
        <ButtonItem
          layout="below"
          bottomSeparator="none"
          disabled={busy || !configured || batch !== null || games.length === 0}
          onClick={() => void runBatch()}
        >
          {batch
            ? t("qam.artwork.batchProgress", { done: batch.done, total: batch.total })
            : t("qam.artwork.applyMissing")}
        </ButtonItem>
      </PanelSectionRow>
      {applied !== null ? (
        <PanelSectionRow>
          <div style={inlineStatusStyle("success")}>
            {t("toast.artwork.applied", { count: applied })}
          </div>
        </PanelSectionRow>
      ) : null}
      <PanelSectionRow>
        <Field focusable={false} childrenLayout="below" padding="none" bottomSeparator="none">
          <div style={compactTextStyle}>{t("qam.artwork.help")}</div>
          {error ? <div style={inlineStatusStyle("error")}>{error}</div> : null}
        </Field>
      </PanelSectionRow>
    </CollapsibleSection>
  );
}
