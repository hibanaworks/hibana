import Hibana.GlobalSemantics

namespace HibanaQuic.OfferRepro
open Hibana

def keyService (base : Nat) : Choreo :=
  .seq (.send 3 1 base 4)
    (.roll (.route .intrinsic
      (.seq (.send 3 1 (base + 1) 4) (.send 1 3 (base + 2) 4))
      (.send 3 1 (base + 3) 4)))

def receiveService : Choreo := .roll (.seq (.send 0 1 10 4) (.send 1 0 11 4))
def transmitService : Choreo := .roll (.seq (.send 2 3 20 4)
  (.seq (.send 3 4 21 4) (.seq (.send 4 3 22 4) (.send 3 2 23 4))))
def timerService : Choreo := .roll (.seq (.send 5 3 30 8) (.send 3 5 31 8))

def choreography : Choreo := .par (.par receiveService (.par transmitService timerService))
  (.par (keyService 70) (.par (keyService 80) (keyService 90)))

#eval choreography.globalEvents.map (fun event => (event.label, event.lane))
#eval choreography.globalRolls.map (fun roll => (roll.events, roll.conflicts))

def trace : List GlobalOperation := [
  .send 8, .recv 8, .send 12, .recv 12, .send 16, .recv 16,
  .send 9, .recv 9, .send 10, .recv 10,
  .send 13, .recv 13, .send 14, .recv 14,
  .roll 3, .send 11, .recv 11,
  .roll 4, .send 15, .recv 15,
  .send 17, .recv 17, .send 18, .recv 18]

def run : GlobalConfig → List GlobalOperation → Option GlobalConfig
  | current, [] => some current
  | current, action :: rest => do
    let next ← current.step? action
    run next rest

def traceAccepted : Bool := (run (GlobalConfig.initial 1 6 choreography) trace).isSome
#eval traceAccepted
set_option maxRecDepth 100000
set_option maxHeartbeats 10000000
 theorem original_trace_is_admitted : traceAccepted = true := by decide
#print axioms original_trace_is_admitted


def minimalService (base : Nat) : Choreo :=
  .seq (.send 0 1 base 4)
    (.roll (.route .intrinsic
      (.seq (.send 0 1 (base + 1) 4) (.send 1 0 (base + 2) 4))
      (.send 0 1 (base + 3) 4)))
def minimalChoreography : Choreo := .par (minimalService 70) (minimalService 90)
def minimalTrace : List GlobalOperation := [
  .send 0, .recv 0, .send 4, .recv 4,
  .send 1, .recv 1, .send 2, .recv 2,
  .roll 0, .send 3, .recv 3, .send 5, .recv 5, .send 6, .recv 6]
def minimalTraceAccepted : Bool :=
  (run (GlobalConfig.initial 1 2 minimalChoreography) minimalTrace).isSome
 theorem minimal_trace_is_admitted : minimalTraceAccepted = true := by decide
#print axioms minimal_trace_is_admitted

def withoutSiblingInstall : List GlobalOperation := [
  .send 0, .recv 0,
  .send 1, .recv 1, .send 2, .recv 2,
  .roll 0, .send 3, .recv 3, .send 5, .recv 5]
def invalidTraceAccepted : Bool :=
  (run (GlobalConfig.initial 1 2 minimalChoreography) withoutSiblingInstall).isSome
 theorem missing_installation_is_rejected : invalidTraceAccepted = false := by decide
#print axioms missing_installation_is_rejected

end HibanaQuic.OfferRepro
