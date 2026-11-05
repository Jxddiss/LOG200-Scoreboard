// File d'attente du ladder (queue.json), dessinée en arbre binaire de recherche équilibré : l'ordre
// infixe est celui de la file, la tête est le nœud le plus à gauche.

const SVG = 'http://www.w3.org/2000/svg'
const COLUMN = 66
const ROW = 58
const NODE = { w: 58, h: 34 }
const LABEL = 9

// Nœuds dans l'ordre de la file : `parent` est un indice, `head` marque le chemin de la racine à la tête.
export function layout(teams) {
    const nodes = teams.map((team) => ({ ...team, depth: 0, parent: null, head: false }))
    const place = (lo, hi, depth, parent) => {
        if (lo >= hi) return
        const mid = (lo + hi - 1) >> 1
        Object.assign(nodes[mid], { depth, parent, head: lo === 0 })
        place(lo, mid, depth + 1, mid)
        place(mid + 1, hi, depth + 1, mid)
    }
    place(0, nodes.length, 0, null)
    return nodes
}

function svg(tag, attrs = {}, text) {
    const node = document.createElementNS(SVG, tag)
    for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v)
    if (text !== undefined) node.textContent = text
    return node
}

const ahead = (n) => `${n < 0 ? '−' : '+'}${Math.abs(n)}`
const short = (id) => (id.length > LABEL ? `${id.slice(0, LABEL - 1)}…` : id)

// shortcut: une colonne par équipe, illisible au-delà d'une trentaine ; passer alors à une liste.
export function drawQueue(teams, busy) {
    const nodes = layout(teams)
    const depth = Math.max(0, ...nodes.map((n) => n.depth))
    const center = (i) => ({ x: i * COLUMN + COLUMN / 2, y: nodes[i].depth * ROW + NODE.h / 2 + 4 })
    const root = svg('svg', {
        viewBox: `0 0 ${nodes.length * COLUMN} ${(depth + 1) * ROW + 12}`,
        width: nodes.length * COLUMN,
        role: 'img',
        'aria-label': `File d'attente : ${teams.map((t) => t.id).join(', ')}`,
    })
    nodes.forEach((node, i) => {
        if (node.parent === null) return
        const from = center(node.parent)
        const to = center(i)
        root.append(
            svg('line', {
                class: node.head ? 'edge on' : 'edge',
                x1: from.x,
                y1: from.y + NODE.h / 2,
                x2: to.x,
                y2: to.y - NODE.h / 2,
            }),
        )
    })
    nodes.forEach((node, i) => {
        const { x, y } = center(i)
        const kind = [node.bot ? 'bot' : 'team', busy.has(node.id) && 'busy', node.head && 'on'].filter(Boolean)
        const group = svg('g', { class: `node ${kind.join(' ')}` })
        group.append(
            svg('title', {}, `${node.id} : ${ahead(node.ahead)}${busy.has(node.id) ? ', en partie' : ''}`),
            svg('rect', { x: x - NODE.w / 2, y: y - NODE.h / 2, width: NODE.w, height: NODE.h, rx: 7 }),
            svg('text', { x, y: y - 3 }, short(node.id)),
            svg('text', { class: 'ahead', x, y: y + 11 }, ahead(node.ahead)),
        )
        if (i === 0) group.append(svg('text', { class: 'mark', x, y: y + NODE.h / 2 + 12 }, '▲'))
        root.append(group)
    })
    return root
}
