import Hibana.StaticProjectability

namespace Hibana

example :
    checkStaticProjectability 3
      (.par (.send 0 1 5 0) (.send 0 2 5 0)) = false := by
  native_decide

example :
    checkStaticProjectability 3
      (.route (.dynamic 7)
        (.seq (.send 0 1 1 0) (.send 0 2 3 0))
        (.seq (.send 0 1 2 0) (.send 0 2 4 0))) = true := by
  native_decide

example :
    checkStaticProjectability 3
      (.par (.send 0 1 5 17) (.send 0 2 5 19)) = true := by
  native_decide

/-- A future sender without a causal handoff cannot share the receive FIFO. -/
example :
    checkStaticProjectability 3
      (.seq (.send 0 2 1 0) (.send 1 2 2 0)) = false := by
  native_decide

/-- A receive followed by an in-band causal chain transfers authority to the
later sender without adding a runtime queue or wire field. -/
example :
    checkStaticProjectability 3
      (.seq (.send 0 2 1 0)
        (.seq (.send 2 1 2 0) (.send 1 2 3 0))) = true := by
  native_decide

/-- A handoff that is valid within one roll body may still be invalid from the
body tail to the next iteration's head. -/
example :
    checkStaticProjectability 3
      (.roll (.seq (.send 0 2 1 0)
        (.seq (.send 2 1 2 0) (.send 1 2 3 0)))) = false := by
  native_decide

/-- Returning authority to the first sender closes the causal cycle without a
wire epoch or an additional runtime queue. -/
example :
    checkStaticProjectability 3
      (.roll (.seq (.send 0 2 1 0)
        (.seq (.send 2 1 2 0)
          (.seq (.send 1 2 3 0) (.send 2 0 4 0))))) = true := by
  native_decide

/-- Opposite route arms from different iterations are not mutually exclusive. -/
example :
    checkStaticProjectability 3
      (.roll (.route (.dynamic 7)
        (.send 0 2 1 0) (.send 1 2 2 0))) = false := by
  native_decide

/-- Causal handoff cannot repair competing route controllers: branch
authority must be unique before iteration reasoning applies. -/
example :
    checkStaticProjectability 3
      (.roll (.route (.dynamic 7)
        (.seq (.send 0 2 1 0)
          (.seq (.send 2 0 3 0) (.send 2 1 4 0)))
        (.seq (.send 1 2 2 0)
          (.seq (.send 2 0 5 0) (.send 2 1 6 0))))) = false := by
  native_decide

/-- Local order does not cross directly between parallel arms. -/
example :
    checkStaticProjectability 3
      (.seq (.par (.send 0 2 1 0) (.send 2 1 2 0))
        (.send 1 2 3 0)) = false := by
  native_decide

/-- An unrelated route arm cannot become causal evidence for endpoints that
remain outside that route. -/
example :
    checkStaticProjectability 5
      (.seq (.send 0 2 1 0)
        (.seq
          (.route (.dynamic 7) (.send 2 1 2 0) (.send 3 4 4 0))
          (.send 1 2 3 0))) = false := by
  native_decide

/-- Mutually exclusive receive occurrences do not authorize competing
first-visible senders at one resolved route. -/
example :
    checkStaticProjectability 3
      (.route (.dynamic 7) (.send 0 2 1 0) (.send 1 2 2 0)) = false := by
  native_decide

/-- Shared local output is not intrinsic branch evidence. Its frame identity
still depends on the globally selected arm. -/
example :
    checkStaticProjectability 4
      (.route .intrinsic
        (.seq (.send 0 2 1 0) (.send 1 3 5 0))
        (.seq (.send 0 2 2 0) (.send 1 3 5 0))) = false := by
  native_decide

example :
    checkStaticProjectability 4
      (.route .intrinsic
        (.seq (.send 0 3 118 0) (.send 1 2 42 0))
        (.seq (.send 0 3 119 0)
          (.seq (.send 1 2 42 0) (.send 1 2 77 0)))) = false := by
  native_decide

/-- Absence is not asynchronous branch evidence. Role 1 cannot know whether to
finish the left arm or wait for the right-arm frame. -/
example :
    checkStaticProjectability 3
      (.route .intrinsic (.send 0 2 1 0) (.send 0 1 2 0)) = false := by
  native_decide

/-- Roll reentry is not branch authority. A passive sender cannot choose the
right arm merely because the enclosing route may repeat. -/
example :
    checkStaticProjectability 3
      (.roll (.route .intrinsic
        (.send 0 2 1 0)
        (.seq (.send 0 2 2 0) (.send 1 2 3 0)))) = false := by
  native_decide

example :
    checkStaticProjectability 3
      (.seq (.roll (.send 0 1 5 0)) (.send 0 2 5 0)) = false := by
  native_decide

example :
    checkStaticProjectability 2
      (.route .intrinsic (.send 0 1 1 0) (.send 0 1 2 0)) = true := by
  native_decide

/-- Source is part of inbound identity, so unrelated senders do not consume
each other's frame-color domain. -/
private def sourceAwareRouteColoring : Choreo :=
  .route .intrinsic (.send 0 2 1 0) (.send 1 2 2 0)

example :
    (sourceAwareRouteColoring.compiledOccurrences.occurrences.map
      CompiledOccurrence.frameLabel) = [0, 0] := by
  native_decide

private def nestedRollFrameColoring : Choreo :=
  .roll (.route .intrinsic
    (.send 0 1 10 0)
    (.seq (.send 0 1 11 0)
      (.route .intrinsic (.send 0 1 12 0) (.send 0 1 13 0))))

/-- Elastic reentry can expose every distinct route path of one inbound key;
the canonical compiler assigns those paths exact, distinct frame colors. -/
example :
    (nestedRollFrameColoring.compiledOccurrences.occurrences.map
      CompiledOccurrence.frameLabel) = [0, 1, 2, 3] := by
  native_decide

example : nestedRollFrameColoring.checkInboundOccurrenceColoring = true := by
  native_decide

private def repeatedInboundEvidence : Nat → Choreo
  | 0 => .send 0 1 1 0
  | count + 1 => .seq (repeatedInboundEvidence count) (.send 0 1 1 0)

example :
    (repeatedInboundEvidence 255).checkInboundOccurrenceColoring = true := by
  native_decide

example :
    (repeatedInboundEvidence 256).checkInboundOccurrenceColoring = true := by
  native_decide

private def transportAdmissionRegressionChoreo : Choreo :=
  .send 0 1 9 4

private def transportAdmissionRegressionFrame
    (session lane : Nat) : TransportFrame := {
  channel := {
    session
    generation := 3
    lane
    sender := 0
    receiver := 1
  }
  sequence := 0
  frameLabel := ⟨0, by omega⟩
}

private def transportAdmissionRegressionConfig : GlobalConfig :=
  (GlobalConfig.initial 7 2 transportAdmissionRegressionChoreo).withPhase 0 (.queued 0)

example :
    (transportAdmissionRegressionConfig.admitTransportFrame?
        (transportAdmissionRegressionFrame 7 0)).isSome = true := by
  native_decide

example :
    (transportAdmissionRegressionConfig.admitTransportFrame?
        (transportAdmissionRegressionFrame 7 1)).isSome = false := by
  native_decide

example :
    (transportAdmissionRegressionConfig.admitTransportFrame?
        (transportAdmissionRegressionFrame 8 0)).isSome = false := by
  native_decide

end Hibana
