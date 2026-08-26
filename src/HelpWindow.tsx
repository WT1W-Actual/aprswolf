import { useEffect, useMemo, useState } from "react";
import { HELP_SECTIONS } from "./helpContent";
import { useTheme } from "./lib/useTheme";
import "./App.css";

function HelpWindow() {
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState(HELP_SECTIONS[0]?.id ?? "overview");

  useTheme();

  const filteredSections = useMemo(() => {
    const term = query.trim().toLowerCase();
    if (!term) return HELP_SECTIONS;
    return HELP_SECTIONS.filter((section) => {
      if (section.title.toLowerCase().includes(term) || section.summary.toLowerCase().includes(term)) {
        return true;
      }
      return section.blocks.some((block) => {
        if (block.type === "paragraph") return block.text.toLowerCase().includes(term);
        if (block.type === "callout") return block.title.toLowerCase().includes(term) || block.text.toLowerCase().includes(term);
        return block.items.some((item) => item.toLowerCase().includes(term)) || (block.title?.toLowerCase().includes(term) ?? false);
      });
    });
  }, [query]);

  useEffect(() => {
    if (!filteredSections.some((section) => section.id === selectedId)) {
      setSelectedId(filteredSections[0]?.id ?? HELP_SECTIONS[0]?.id ?? "overview");
    }
  }, [filteredSections, selectedId]);

  const activeSection = filteredSections.find((section) => section.id === selectedId) ?? filteredSections[0] ?? HELP_SECTIONS[0];

  async function closeWindow() {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().destroy();
      return;
    } catch {
      window.close();
    }
  }

  return (
    <div className="help-window">
      <div className="help-window-toolbar">
        <div>
          <div className="help-window-title">Help</div>
          <div className="help-window-subtitle">APRSWolf User Guide</div>
        </div>
        <label className="help-search">
          <span>Search</span>
          <input
            value={query}
            onChange={(event) => setQuery(event.currentTarget.value)}
            placeholder="Search help"
            aria-label="Search help"
          />
        </label>
        <button type="button" className="received-window-close" onClick={closeWindow}>Close Window</button>
      </div>

      <div className="help-window-body">
        <aside className="help-sidebar" aria-label="Help topics">
          {filteredSections.map((section) => (
            <button
              key={section.id}
              type="button"
              className={`help-nav-item ${section.id === activeSection?.id ? "active" : ""}`}
              onClick={() => setSelectedId(section.id)}
            >
              <span className="help-nav-title">{section.title}</span>
              <span className="help-nav-summary">{section.summary}</span>
            </button>
          ))}
          {filteredSections.length === 0 ? (
            <p className="help-empty">No help topics match that search.</p>
          ) : null}
        </aside>

        <main className="help-content" aria-live="polite">
          {activeSection ? (
            <>
              <header className="help-hero">
                <p className="help-kicker">APRSWolf Help</p>
                <h1>{activeSection.title}</h1>
                <p>{activeSection.summary}</p>
              </header>

              {activeSection.blocks.map((block, index) => {
                if (block.type === "paragraph") {
                  return <section key={`${activeSection.id}-${index}`} className="help-card"><p>{block.text}</p></section>;
                }

                if (block.type === "callout") {
                  return (
                    <section key={`${activeSection.id}-${index}`} className={`help-card help-callout ${block.tone}`}>
                      <h2>{block.title}</h2>
                      <p>{block.text}</p>
                    </section>
                  );
                }

                const ListTag = block.type === "steps" ? "ol" : "ul";
                return (
                  <section key={`${activeSection.id}-${index}`} className="help-card">
                    {block.title ? <h2>{block.title}</h2> : null}
                    <ListTag className="help-list">
                      {block.items.map((item) => <li key={item}>{item}</li>)}
                    </ListTag>
                  </section>
                );
              })}
            </>
          ) : (
            <section className="help-card"><p>Select a help topic.</p></section>
          )}
        </main>
      </div>
    </div>
  );
}

export default HelpWindow;