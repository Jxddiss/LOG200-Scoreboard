// File d'attente du ladder (queue.json), dessinée comme un plateau : un arbre binaire de recherche
// équilibré dont l'ordre infixe est celui de la file. La prochaine équipe libre est la case rouge.

const SVG = 'http://www.w3.org/2000/svg'
const COLUMN = 72
const ROW = 70
const TILE = { w: 64, h: 40 }
const TOP = 10
const BELOW = 54
const LABEL = 9
const BEND = 7
const STEP_MS = 90

// Nœuds dans l'ordre de la file : `parent` est un indice, `path` marque le chemin de la racine à `next`.
export function layout(teams, next) {
    const nodes = teams.map((team) => ({ ...team, depth: 0, parent: null, path: false }))
    const place = (lo, hi, depth, parent) => {
        if (lo >= hi) return
        const mid = (lo + hi - 1) >> 1
        Object.assign(nodes[mid], { depth, parent, path: lo <= next && next < hi })
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

const signed = (n) => `${n < 0 ? '−' : '+'}${Math.abs(n)}`
const short = (id) => (id.length > LABEL ? `${id.slice(0, LABEL - 1)}…` : id)

// Coude arrondi de `a` vers `b`, depuis un point déjà placé à l'abscisse de `a`, jusqu'à l'ordonnée `to`.
function bend(a, b, to) {
    const mid = (a.y + TILE.h / 2 + b.y - TILE.h / 2) / 2
    const s = Math.sign(b.x - a.x)
    return `V${mid - BEND}Q${a.x} ${mid} ${a.x + s * BEND} ${mid}H${b.x - s * BEND}Q${b.x} ${mid} ${b.x} ${mid + BEND}V${to}`
}

function tile(node, { x, y }, pawn, next) {
    const kind = [node.bot ? 'bot' : 'team', node.path && 'path', next && 'next', pawn && 'busy']
    const group = svg('g', {
        class: `node ${kind.filter(Boolean).join(' ')}`,
        'data-team': node.id,
        style: `--d: ${node.depth}`,
    })
    const where = pawn ? `, en partie avec les ${pawn === 'red' ? 'rouges' : 'noirs'}` : ''
    group.append(
        svg('title', {}, `${node.id} : ${signed(node.ahead)}${where}`),
        svg('rect', { x: x - TILE.w / 2, y: y - TILE.h / 2, width: TILE.w, height: TILE.h, rx: 5 }),
        svg('text', { class: 'name', x, y: y - 2 }, short(node.id)),
        svg('text', { class: 'ahead', x, y: y + 12 }, signed(node.ahead)),
    )
    if (pawn) group.append(svg('circle', { class: `pawn ${pawn}`, cx: x + TILE.w / 2 - 4, cy: y - TILE.h / 2 + 4, r: 6 }))
    if (next) group.append(svg('text', { class: 'next-label', x, y: y + TILE.h / 2 + 15 }, 'prochaine'))
    return group
}

// shortcut: une colonne par équipe, l'arbre défile au-delà d'une quinzaine ; passer à une liste vers 60.
function draw(teams, busy, next) {
    const nodes = layout(teams, next)
    const depth = Math.max(...nodes.map((n) => n.depth))
    const width = nodes.length * COLUMN
    const height = TOP + depth * ROW + TILE.h + BELOW
    const at = new Map(nodes.map((n, i) => [n.id, { x: i * COLUMN + COLUMN / 2, y: TOP + n.depth * ROW + TILE.h / 2 }]))
    const center = (i) => at.get(nodes[i].id)
    const root = svg('svg', {
        viewBox: `0 0 ${width} ${height}`,
        width,
        height,
        role: 'img',
        'aria-label': `File d'attente, dans l'ordre : ${teams.map((t) => t.id).join(', ')}`,
    })
    const reveal = (depth + 1) * STEP_MS + 120
    root.style.setProperty('--trail', `${reveal}ms`)
    root.style.setProperty('--ignite', `${reveal + 480}ms`)

    const edges = svg('g', { class: 'edges' })
    nodes.forEach((node, i) => {
        if (node.parent === null) return
        const a = center(node.parent)
        const b = center(i)
        edges.append(
            svg('path', {
                class: 'edge',
                d: `M${a.x} ${a.y + TILE.h / 2}${bend(a, b, b.y - TILE.h / 2)}`,
                pathLength: 1,
                style: `--d: ${node.depth - 1}`,
            }),
        )
    })
    root.append(edges)

    const chain = []
    for (let i = next; i !== null; i = nodes[i].parent) chain.unshift(i)
    if (chain.length > 1) {
        let d = `M${center(chain[0]).x} ${center(chain[0]).y}`
        for (let k = 1; k < chain.length; k++) d += bend(center(chain[k - 1]), center(chain[k]), center(chain[k]).y)
        root.append(svg('path', { class: 'trail', d, pathLength: 1 }))
    }

    nodes.forEach((node, i) => root.append(tile(node, center(i), busy.get(node.id), i === next)))

    const axis = height - 16
    root.append(
        svg('path', { class: 'axis', d: `M8 ${axis}H${width - 8}M${width - 13} ${axis - 4}L${width - 8} ${axis}L${width - 13} ${axis + 4}` }),
        svg('text', { class: 'axis-label', x: 8, y: axis + 13 }, 'en retard'),
        svg('text', { class: 'axis-label end', x: width - 8, y: axis + 13 }, 'en avance'),
    )
    return { root, at }
}

// Fait glisser chaque case de son ancienne place vers la nouvelle : la file avance sous les yeux.
function slide(root, before, at) {
    const moved = []
    for (const group of root.querySelectorAll('.node')) {
        const team = group.getAttribute('data-team')
        const from = before.get(team)
        if (!from) {
            group.classList.add('fresh')
            continue
        }
        const to = at.get(team)
        group.style.transition = 'none'
        group.style.transform = `translate(${from.x - to.x}px, ${from.y - to.y}px)`
        moved.push(group)
    }
    root.getBoundingClientRect()
    for (const group of moved) {
        group.style.transition = ''
        group.style.transform = ''
    }
    root.classList.add('moving')
}

function sentence(names) {
    const strong = names.map((name) => Object.assign(document.createElement('strong'), { textContent: name }))
    const parts = []
    strong.forEach((node, i) => {
        if (i > 0) parts.push(i === strong.length - 1 ? ' et ' : ', ')
        parts.push(node)
    })
    return parts
}

// `tree` reçoit l'arbre, `lede` la phrase d'en-tête. `reset()` rejoue l'entrée à la prochaine ouverture.
export function createQueueView({ tree, lede }) {
    let key = null
    let positions = null
    return {
        reset() {
            key = null
            positions = null
        },
        show(teams, running) {
            const busy = new Map(running.flatMap((g) => [[g.red, 'red'], [g.black, 'black']]))
            const now = JSON.stringify([teams, [...busy]])
            if (now === key) return
            key = now
            if (!teams?.length) {
                positions = null
                lede.replaceChildren()
                const text = teams ? 'Aucune équipe en file pour le moment.' : "Ce ladder ne publie pas encore sa file d'attente."
                tree.replaceChildren(Object.assign(document.createElement('p'), { className: 'queue-empty', textContent: text }))
                return
            }
            const free = teams.filter((t) => !busy.has(t.id))
            const next = Math.max(0, teams.indexOf(free[0]))
            const games = running.length
            lede.replaceChildren(
                ...(free.length
                    ? ['Prochaines à jouer : ', ...sentence(free.slice(0, 3).map((t) => t.id)), '.']
                    : ['Toutes les équipes sont en partie.']),
                games ? ` ${games} partie${games > 1 ? 's' : ''} en cours.` : '',
            )
            const { root, at } = draw(teams, busy, next)
            const before = positions
            positions = at
            tree.replaceChildren(root)
            if (before) slide(root, before, at)
            else root.classList.add('enter')
        },
    }
}
