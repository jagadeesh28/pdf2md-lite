import React, { useState, useEffect, useRef } from 'react';
import {
  FileText,
  FolderOpen,
  Eye,
  Settings,
  HelpCircle,
  Play,
  RotateCcw,
  Sparkles,
  Cpu,
  Layers,
  Terminal,
  X,
  FileCode,
  Image,
  Database,
} from 'lucide-react';

interface MenuBarProps {
  onOpenPdf: () => void;
  onOpenDirectory: () => void;
  onWatchDirectory: () => void;
  onOpenVault: () => void;
  onExportMarkdown: () => void;
  onStartConversion: () => void;
  onDryRun: () => void;
  onStopConversion: () => void;
  onFormatNotes: () => void;
  onFixLatex: () => void;
  onConvertPngToJpeg: () => void;
  onClearCompleted: () => void;
  onClearAll: () => void;
  onEmbedVault: () => void;
  onOpenSettings: () => void;
  onOpenModels: () => void;
  onOpenShortcuts: () => void;
  onOpenAbout: () => void;
  activeView: 'queue' | 'inspector';
  onToggleView: (view: 'queue' | 'inspector') => void;
  showConsole: boolean;
  onToggleConsole: () => void;
  theme: 'dark' | 'obsidian' | 'light';
  onToggleTheme: () => void;
  isRunning: boolean;
}

export const MenuBar: React.FC<MenuBarProps> = ({
  onOpenPdf,
  onOpenDirectory,
  onWatchDirectory,
  onOpenVault,
  onExportMarkdown,
  onStartConversion,
  onDryRun,
  onStopConversion,
  onFormatNotes,
  onFixLatex,
  onConvertPngToJpeg,
  onClearCompleted,
  onClearAll,
  onEmbedVault,
  onOpenSettings,
  onOpenModels,
  onOpenShortcuts,
  onOpenAbout,
  activeView,
  onToggleView,
  showConsole,
  onToggleConsole,
  theme,
  onToggleTheme,
  isRunning,
}) => {
  const [openMenu, setOpenMenu] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  // Close menus on outside click
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setOpenMenu(null);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  // Global keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Ctrl+O: Open PDF
      if (e.ctrlKey && !e.shiftKey && e.key.toLowerCase() === 'o') {
        e.preventDefault();
        onOpenPdf();
      }
      // Ctrl+Shift+O: Open Directory
      if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 'o') {
        e.preventDefault();
        onOpenDirectory();
      }
      // Ctrl+D: Dry Run
      if (e.ctrlKey && e.key.toLowerCase() === 'd') {
        e.preventDefault();
        onDryRun();
      }
      // Ctrl+Enter: Start Conversion
      if (e.ctrlKey && e.key === 'Enter') {
        e.preventDefault();
        onStartConversion();
      }
      // Ctrl+1: Inspector view
      if (e.ctrlKey && e.key === '1') {
        e.preventDefault();
        onToggleView('inspector');
      }
      // Ctrl+2: Queue view
      if (e.ctrlKey && e.key === '2') {
        e.preventDefault();
        onToggleView('queue');
      }
      // Ctrl+`: Toggle console
      if (e.ctrlKey && e.key === '`') {
        e.preventDefault();
        onToggleConsole();
      }
      // Ctrl+,: Settings
      if (e.ctrlKey && e.key === ',') {
        e.preventDefault();
        onOpenSettings();
      }
      // F1: Shortcuts
      if (e.key === 'F1') {
        e.preventDefault();
        onOpenShortcuts();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [
    onOpenPdf,
    onOpenDirectory,
    onDryRun,
    onStartConversion,
    onToggleView,
    onToggleConsole,
    onOpenSettings,
    onOpenShortcuts,
  ]);

  const toggleMenu = (name: string) => {
    setOpenMenu((prev) => (prev === name ? null : name));
  };

  const handleAction = (action: () => void) => {
    setOpenMenu(null);
    action();
  };

  return (
    <div
      ref={menuRef}
      className="bg-gray-950 border-b border-gray-800 text-xs text-gray-300 flex items-center justify-between px-3 h-9 select-none z-50 relative"
    >
      <div className="flex items-center space-x-1">
        {/* App Logo / Title */}
        <div className="flex items-center space-x-1.5 mr-3 font-semibold text-white tracking-wide">
          <span className="w-2.5 h-2.5 rounded-full bg-purple-500 animate-pulse"></span>
          <span>pdfmd-lite</span>
        </div>

        {/* FILE MENU */}
        <div className="relative">
          <button
            onClick={() => toggleMenu('file')}
            className={`px-2.5 py-1 rounded transition-colors ${
              openMenu === 'file' ? 'bg-gray-800 text-white' : 'hover:bg-gray-800/70 hover:text-white'
            }`}
          >
            File
          </button>
          {openMenu === 'file' && (
            <div className="absolute left-0 top-full mt-1 w-56 bg-gray-900 border border-gray-700 rounded-md shadow-xl py-1 z-50">
              <MenuItem
                icon={<FileText className="w-4 h-4 text-purple-400" />}
                label="Open PDF(s)..."
                shortcut="Ctrl+O"
                onClick={() => handleAction(onOpenPdf)}
              />
              <MenuItem
                icon={<FolderOpen className="w-4 h-4 text-blue-400" />}
                label="Open Directory (Batch)..."
                shortcut="Ctrl+Shift+O"
                onClick={() => handleAction(onOpenDirectory)}
              />
              <MenuItem
                icon={<Eye className="w-4 h-4 text-emerald-400" />}
                label="Watch Directory..."
                onClick={() => handleAction(onWatchDirectory)}
              />
              <div className="border-t border-gray-800 my-1" />
              <MenuItem
                icon={<FolderOpen className="w-4 h-4 text-amber-400" />}
                label="Open Output Vault..."
                shortcut="Ctrl+Shift+V"
                onClick={() => handleAction(onOpenVault)}
              />
              <MenuItem
                icon={<FileCode className="w-4 h-4 text-indigo-400" />}
                label="Export Markdown..."
                shortcut="Ctrl+S"
                onClick={() => handleAction(onExportMarkdown)}
              />
              <div className="border-t border-gray-800 my-1" />
              <MenuItem
                icon={<X className="w-4 h-4 text-red-400" />}
                label="Exit"
                shortcut="Alt+F4"
                onClick={() => window.close()}
              />
            </div>
          )}
        </div>

        {/* EDIT MENU */}
        <div className="relative">
          <button
            onClick={() => toggleMenu('edit')}
            className={`px-2.5 py-1 rounded transition-colors ${
              openMenu === 'edit' ? 'bg-gray-800 text-white' : 'hover:bg-gray-800/70 hover:text-white'
            }`}
          >
            Edit
          </button>
          {openMenu === 'edit' && (
            <div className="absolute left-0 top-full mt-1 w-64 bg-gray-900 border border-gray-700 rounded-md shadow-xl py-1 z-50">
              <MenuItem
                icon={<Sparkles className="w-4 h-4 text-yellow-400" />}
                label="Format Vault Notes"
                onClick={() => handleAction(onFormatNotes)}
              />
              <MenuItem
                icon={<FileCode className="w-4 h-4 text-cyan-400" />}
                label="Sanitize LaTeX Delimiters"
                onClick={() => handleAction(onFixLatex)}
              />
              <MenuItem
                icon={<Image className="w-4 h-4 text-pink-400" />}
                label="Convert PNGs to JPEG"
                onClick={() => handleAction(onConvertPngToJpeg)}
              />
              <div className="border-t border-gray-800 my-1" />
              <MenuItem
                label="Clear Completed Jobs"
                onClick={() => handleAction(onClearCompleted)}
              />
              <MenuItem
                label="Clear All Jobs"
                onClick={() => handleAction(onClearAll)}
              />
            </div>
          )}
        </div>

        {/* CONVERT MENU */}
        <div className="relative">
          <button
            onClick={() => toggleMenu('convert')}
            className={`px-2.5 py-1 rounded transition-colors ${
              openMenu === 'convert' ? 'bg-gray-800 text-white' : 'hover:bg-gray-800/70 hover:text-white'
            }`}
          >
            Convert
          </button>
          {openMenu === 'convert' && (
            <div className="absolute left-0 top-full mt-1 w-60 bg-gray-900 border border-gray-700 rounded-md shadow-xl py-1 z-50">
              <MenuItem
                icon={<Play className="w-4 h-4 text-emerald-400" />}
                label="Start Conversion"
                shortcut="Ctrl+Enter"
                disabled={isRunning}
                onClick={() => handleAction(onStartConversion)}
              />
              <MenuItem
                icon={<Eye className="w-4 h-4 text-blue-400" />}
                label="Dry-Run Preview"
                shortcut="Ctrl+D"
                onClick={() => handleAction(onDryRun)}
              />
              <MenuItem
                icon={<RotateCcw className="w-4 h-4 text-amber-400" />}
                label="Stop / Cancel"
                disabled={!isRunning}
                onClick={() => handleAction(onStopConversion)}
              />
              <div className="border-t border-gray-800 my-1" />
              <MenuItem
                icon={<Database className="w-4 h-4 text-purple-400" />}
                label="Retroactively Embed Vault"
                onClick={() => handleAction(onEmbedVault)}
              />
            </div>
          )}
        </div>

        {/* SETTINGS MENU */}
        <div className="relative">
          <button
            onClick={() => toggleMenu('settings')}
            className={`px-2.5 py-1 rounded transition-colors ${
              openMenu === 'settings' ? 'bg-gray-800 text-white' : 'hover:bg-gray-800/70 hover:text-white'
            }`}
          >
            Settings
          </button>
          {openMenu === 'settings' && (
            <div className="absolute left-0 top-full mt-1 w-60 bg-gray-900 border border-gray-700 rounded-md shadow-xl py-1 z-50">
              <MenuItem
                icon={<Settings className="w-4 h-4 text-gray-300" />}
                label="Engine Configuration..."
                shortcut="Ctrl+,"
                onClick={() => handleAction(onOpenSettings)}
              />
              <MenuItem
                icon={<Cpu className="w-4 h-4 text-indigo-400" />}
                label="Provisioning Manager..."
                onClick={() => handleAction(onOpenModels)}
              />
            </div>
          )}
        </div>

        {/* VIEW MENU */}
        <div className="relative">
          <button
            onClick={() => toggleMenu('view')}
            className={`px-2.5 py-1 rounded transition-colors ${
              openMenu === 'view' ? 'bg-gray-800 text-white' : 'hover:bg-gray-800/70 hover:text-white'
            }`}
          >
            View
          </button>
          {openMenu === 'view' && (
            <div className="absolute left-0 top-full mt-1 w-56 bg-gray-900 border border-gray-700 rounded-md shadow-xl py-1 z-50">
              <MenuItem
                icon={<Layers className="w-4 h-4 text-purple-400" />}
                label={`Batch Queue ${activeView === 'queue' ? '✓' : ''}`}
                shortcut="Ctrl+2"
                onClick={() => handleAction(() => onToggleView('queue'))}
              />
              <MenuItem
                icon={<Eye className="w-4 h-4 text-blue-400" />}
                label={`Split-Pane Inspector ${activeView === 'inspector' ? '✓' : ''}`}
                shortcut="Ctrl+1"
                onClick={() => handleAction(() => onToggleView('inspector'))}
              />
              <div className="border-t border-gray-800 my-1" />
              <MenuItem
                icon={<Terminal className="w-4 h-4 text-emerald-400" />}
                label={`Live Log Console ${showConsole ? '✓' : ''}`}
                shortcut="Ctrl+`"
                onClick={() => handleAction(onToggleConsole)}
              />
              <MenuItem
                label={`Theme: ${theme.toUpperCase()}`}
                onClick={() => handleAction(onToggleTheme)}
              />
            </div>
          )}
        </div>

        {/* HELP MENU */}
        <div className="relative">
          <button
            onClick={() => toggleMenu('help')}
            className={`px-2.5 py-1 rounded transition-colors ${
              openMenu === 'help' ? 'bg-gray-800 text-white' : 'hover:bg-gray-800/70 hover:text-white'
            }`}
          >
            Help
          </button>
          {openMenu === 'help' && (
            <div className="absolute left-0 top-full mt-1 w-52 bg-gray-900 border border-gray-700 rounded-md shadow-xl py-1 z-50">
              <MenuItem
                icon={<HelpCircle className="w-4 h-4 text-cyan-400" />}
                label="Keyboard Shortcuts"
                shortcut="F1"
                onClick={() => handleAction(onOpenShortcuts)}
              />
              <div className="border-t border-gray-800 my-1" />
              <MenuItem
                label="About pdfmd-lite"
                onClick={() => handleAction(onOpenAbout)}
              />
            </div>
          )}
        </div>
      </div>

      {/* Right status badge */}
      <div className="flex items-center space-x-3 text-gray-400">
        {isRunning ? (
          <span className="flex items-center space-x-1.5 text-emerald-400 font-medium">
            <span className="w-2 h-2 rounded-full bg-emerald-500 animate-ping"></span>
            <span>Converting...</span>
          </span>
        ) : (
          <span className="text-gray-500">Ready</span>
        )}
      </div>
    </div>
  );
};

interface MenuItemProps {
  icon?: React.ReactNode;
  label: string;
  shortcut?: string;
  disabled?: boolean;
  onClick: () => void;
}

const MenuItem: React.FC<MenuItemProps> = ({ icon, label, shortcut, disabled, onClick }) => {
  return (
    <button
      disabled={disabled}
      onClick={onClick}
      className={`w-full flex items-center justify-between px-3 py-1.5 text-left transition-colors ${
        disabled
          ? 'opacity-40 cursor-not-allowed text-gray-500'
          : 'hover:bg-purple-600/30 hover:text-white text-gray-300'
      }`}
    >
      <div className="flex items-center space-x-2">
        {icon && <span className="flex-shrink-0">{icon}</span>}
        <span>{label}</span>
      </div>
      {shortcut && <span className="text-[10px] text-gray-500 ml-4 font-mono">{shortcut}</span>}
    </button>
  );
};
