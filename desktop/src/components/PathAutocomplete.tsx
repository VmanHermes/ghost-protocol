import { useCallback, useEffect, useRef, useState } from "react";
import { listDirs } from "../api";

type Props = {
  value: string;
  onChange: (value: string) => void;
  baseUrl: string;
  placeholder?: string;
  autoFocus?: boolean;
  style?: React.CSSProperties;
};

export function PathAutocomplete({ value, onChange, baseUrl, placeholder, autoFocus, style }: Props) {
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [displayParent, setDisplayParent] = useState("");
  const [highlightIndex, setHighlightIndex] = useState(-1);
  const [open, setOpen] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const blurTimeoutRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const requestIdRef = useRef(0);

  const fetchSuggestions = useCallback(
    (path: string) => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
      debounceRef.current = setTimeout(async () => {
        const id = ++requestIdRef.current;
        try {
          const result = await listDirs(baseUrl, path);
          if (id !== requestIdRef.current) return;
          setSuggestions(result.dirs);
          setDisplayParent(result.parent);
          setHighlightIndex(-1);
          setOpen(result.dirs.length > 0);
        } catch {
          if (id !== requestIdRef.current) return;
          setSuggestions([]);
          setOpen(false);
        }
      }, 150);
    },
    [baseUrl],
  );

  useEffect(() => {
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
      if (blurTimeoutRef.current) clearTimeout(blurTimeoutRef.current);
    };
  }, []);

  const handleChange = useCallback(
    (newValue: string) => {
      onChange(newValue);
      fetchSuggestions(newValue);
    },
    [onChange, fetchSuggestions],
  );

  const selectSuggestion = useCallback(
    (dir: string) => {
      const parent = displayParent.replace(/\/+$/, "");
      const fullPath = parent ? `${parent}/${dir}/` : `${dir}/`;
      onChange(fullPath);
      setOpen(false);
      fetchSuggestions(fullPath);
    },
    [displayParent, onChange, fetchSuggestions],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (!open || suggestions.length === 0) return;

      if (e.key === "ArrowDown") {
        e.preventDefault();
        setHighlightIndex((prev) => (prev + 1) % suggestions.length);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setHighlightIndex((prev) => (prev <= 0 ? suggestions.length - 1 : prev - 1));
      } else if (e.key === "Enter" && highlightIndex >= 0) {
        e.preventDefault();
        selectSuggestion(suggestions[highlightIndex]);
      } else if (e.key === "Escape") {
        setOpen(false);
      } else if (e.key === "Tab" && highlightIndex >= 0) {
        e.preventDefault();
        selectSuggestion(suggestions[highlightIndex]);
      }
    },
    [open, suggestions, highlightIndex, selectSuggestion],
  );

  const handleBlur = useCallback(() => {
    blurTimeoutRef.current = setTimeout(() => setOpen(false), 200);
  }, []);

  const handleFocus = () => {
    if (blurTimeoutRef.current) clearTimeout(blurTimeoutRef.current);
    if (value) fetchSuggestions(value);
  };

  return (
    <div className="path-autocomplete">
      <input
        type="text"
        value={value}
        onChange={(e) => handleChange(e.target.value)}
        onKeyDown={handleKeyDown}
        onBlur={handleBlur}
        onFocus={handleFocus}
        placeholder={placeholder}
        autoFocus={autoFocus}
        style={style}
      />
      {open && suggestions.length > 0 && (
        <ul className="path-autocomplete-dropdown">
          {suggestions.map((dir, i) => (
            <li
              key={dir}
              className={i === highlightIndex ? "path-autocomplete-highlighted" : ""}
              onMouseDown={(e) => {
                e.preventDefault();
                selectSuggestion(dir);
              }}
              onMouseEnter={() => setHighlightIndex(i)}
            >
              <span className="path-autocomplete-parent">{displayParent}/</span>
              {dir}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
