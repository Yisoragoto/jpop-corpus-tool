import { Avatar } from "./Avatar";
import type { Graph } from "../graph";
import { ROLE_LABELS } from "../api";
import { tr } from "../i18n";

/** A stable neighbourhood, with real identities and no colliding canvas labels. */
export function CompactRelations({ graph, focusId, onOpen, onShowAll }: {
  graph: Graph; focusId: number; onOpen: (id: number) => void; onShowAll: () => void;
}) {
  const focus = graph.nodes.findIndex(n => n.id === focusId);
  const center = graph.nodes[focus];
  if (!center) return null;
  const peers = graph.links.filter(l => l.s === focus || l.t === focus)
    .sort((a,b) => Number(b.member) - Number(a.member) || b.shared - a.shared)
    .map(link => ({ link, person: graph.nodes[link.s === focus ? link.t : link.s]! }));
  const shown = peers.slice(0, 4);
  return <div className="relation-neighbourhood">
    <div className="relation-center"><Avatar path={center.imagePath} name={center.name} size={48} /><strong>{center.name}</strong></div>
    <div className="relation-branches" aria-hidden="true"><i /><i /></div>
    <div className="relation-people">
      {shown.map(({ person, link }) => <button type="button" key={person.id} className="relation-person" onClick={() => onOpen(person.id)}>
        <Avatar path={person.imagePath} name={person.name} size={36} />
        <span><strong>{person.name}</strong><small>{link.member ? (person.isBand ? tr("所属乐队") : tr("成员")) : ROLE_LABELS[person.role] ?? person.role}{link.shared > 0 && tr(" · 共 {n} 首", { n: link.shared })}</small></span>
      </button>)}
    </div>
    {peers.length > shown.length && <button type="button" className="link-btn relation-more" onClick={onShowAll}>{tr("查看全部 {n} 位关联人物", { n: peers.length })}</button>}
  </div>;
}
