import { useId } from 'react';
import { Icon, type IconName } from './Icons';

/** A titled section whose body can be collapsed from its header. */
export function Collapsible({ title, count, collapsed, onToggle, badge, icon, className, name, children }: {
  title: string;
  count: number;
  collapsed: boolean;
  onToggle: () => void;
  /** Shown next to the title, even while collapsed. */
  badge?: React.ReactNode;
  /** Shown before the title. */
  icon?: IconName;
  className: string;
  /** Identifies the section to code that measures it (`data-section`). */
  name?: string;
  children: React.ReactNode;
}) {
  const bodyId = useId();
  return (
    <section className={className} data-section={name}>
      <div className="section-header">
        <h3 className="widget-title">
          <button
            type="button"
            className="section-toggle"
            aria-expanded={!collapsed}
            aria-controls={bodyId}
            onClick={onToggle}
          >
            {icon && <Icon name={icon} size={12} className="section-icon" />}
            {title} <span className="count">{count}</span>
            <Icon name={collapsed ? 'chevronRight' : 'chevronDown'} size={12} className="chevron" />
          </button>
          {badge}
        </h3>
      </div>
      <div id={bodyId} hidden={collapsed}>
        {!collapsed && children}
      </div>
    </section>
  );
}
