import React, { useEffect } from 'react';

export function ContextMenu({ menu, onClose }) {
  useEffect(() => {
    if (!menu) return undefined;
    const handleClick = () => onClose?.();
    const handleKey = e => {
      if (e.key === 'Escape') onClose?.();
    };
    window.addEventListener('click', handleClick);
    window.addEventListener('keydown', handleKey);
    return () => {
      window.removeEventListener('click', handleClick);
      window.removeEventListener('keydown', handleKey);
    };
  }, [menu, onClose]);

  if (!menu || !Array.isArray(menu.actions) || menu.actions.length === 0) return null;

  const left = Math.min(menu.x, window.innerWidth - 220);
  const top = Math.min(menu.y, window.innerHeight - (menu.actions.length * 38 + 24));

  return (
    <div
      className="sf-context-menu"
      style={{ left: Math.max(8, left), top: Math.max(8, top) }}
      role="menu"
      onClick={e => e.stopPropagation()}
    >
      {menu.title && <div className="sf-context-title">{menu.title}</div>}
      {menu.actions.map((action, idx) => (
        <button
          key={idx}
          type="button"
          role="menuitem"
          disabled={action.disabled}
          className={`sf-context-item ${action.danger ? 'is-danger' : ''}`}
          onClick={() => {
            onClose?.();
            action.onClick?.();
          }}
        >
          {action.label}
        </button>
      ))}
    </div>
  );
}
