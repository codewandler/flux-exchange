/** A value-free ticket that prevents an older plan/apply answer replacing a newer selection. */
export interface ConnectionRequestTicket {
  generation: number
  connector: string
  selection: string | null
}

/** Monotonic request guard; it holds connector labels, never submitted field values. */
export class LatestConnectionRequest {
  private generation = 0

  begin(connector: string, selection: string | null): ConnectionRequestTicket {
    return { generation: ++this.generation, connector, selection }
  }

  invalidate(): void {
    ++this.generation
  }

  admits(ticket: ConnectionRequestTicket, connector: string | null, selection: string | null): boolean {
    return ticket.generation === this.generation &&
      ticket.connector === connector && ticket.selection === selection
  }
}
