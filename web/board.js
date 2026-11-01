// Plateau 12 × 12 d'une position de positions() : cases, pions, et le dernier coup marqué.

import { el } from './common.js'
import { BLACK, DESTROYED, RED, SIZE } from './replay.js'

export function renderBoard(board, step) {
    const nodes = []
    for (let y = 0; y < SIZE; y++) {
        nodes.push(el('div', { class: 'label' }, String(SIZE - y)))
        for (let x = 0; x < SIZE; x++) {
            const i = y * SIZE + x
            const v = step.cells[i]
            const cell = el('div', { class: 'cell' })
            if (v === DESTROYED) cell.classList.add('hole')
            if (step.move) {
                if (i === step.move.from) cell.classList.add('from')
                if (i === step.move.to) cell.classList.add('to')
                if (i === step.move.destroyed) cell.classList.add('destroyed-now')
            }
            if (v === RED) cell.append(el('div', { class: 'pawn red' }))
            if (v === BLACK) cell.append(el('div', { class: 'pawn black' }))
            nodes.push(cell)
        }
    }
    nodes.push(el('div'))
    for (let x = 0; x < SIZE; x++) nodes.push(el('div', { class: 'label' }, String.fromCharCode(65 + x)))
    board.replaceChildren(...nodes)
}
