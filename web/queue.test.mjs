import { test } from 'node:test'
import assert from 'node:assert/strict'

import { layout } from './queue.js'

const teams = (n) => Array.from({ length: n }, (_, i) => ({ id: `t${i}`, ahead: i, bot: false }))

test('15 équipes forment un arbre parfait, la médiane à la racine', () => {
    const nodes = layout(teams(15))
    assert.deepEqual(
        nodes.map((n) => n.depth),
        [3, 2, 3, 1, 3, 2, 3, 0, 3, 2, 3, 1, 3, 2, 3],
    )
    assert.equal(nodes[7].parent, null)
    assert.deepEqual([nodes[3].parent, nodes[11].parent], [7, 7])
})

test('le chemin de la racine à la tête de file est marqué', () => {
    const nodes = layout(teams(15))
    assert.deepEqual(
        nodes.flatMap((n, i) => (n.head ? [i] : [])),
        [0, 1, 3, 7],
    )
})

test('chaque nœud garde son équipe et sa place dans la file', () => {
    const nodes = layout(teams(4))
    assert.deepEqual(
        nodes.map((n) => n.id),
        ['t0', 't1', 't2', 't3'],
    )
    assert.deepEqual(
        nodes.map((n) => n.parent),
        [1, null, 1, 2],
    )
})

test('une file vide ou seule', () => {
    assert.deepEqual(layout([]), [])
    assert.deepEqual(layout(teams(1)), [{ id: 't0', ahead: 0, bot: false, depth: 0, parent: null, head: true }])
})
