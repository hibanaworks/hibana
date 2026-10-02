#![allow(long_running_const_eval, dead_code)]
use std::{future::Future, pin::pin, sync::{Arc,atomic::{AtomicBool,AtomicUsize,Ordering}}, task::{Wake,Waker,Context,Poll}, time::Instant};
use hibana::{g, runtime::{SessionKitStorage,ids::SessionId,program::project}};
#[path="../hibana-quic/src/carrier.rs"] mod carrier;
#[path="../hibana-quic/src/runtime.rs"] mod runtime;
#[path="../hibana-quic/src/roles/protocol_tls.rs"] mod tls;
#[path="../hibana-quic/src/roles/protocol.rs"] mod key;
struct Flag { ready: AtomicBool, wakes: AtomicUsize }
impl Wake for Flag { fn wake(self: Arc<Self>) { self.wake_by_ref() } fn wake_by_ref(self: &Arc<Self>) { self.ready.store(true,Ordering::Relaxed); self.wakes.fetch_add(1,Ordering::Relaxed); } }
fn drive<F: Future>(f: F) -> (F::Output,usize,usize) { let flag=Arc::new(Flag{ready:AtomicBool::new(true),wakes:AtomicUsize::new(0)}); let waker=Waker::from(flag.clone()); let mut cx=Context::from_waker(&waker);let mut f=pin!(f);let mut polls=0;loop { assert!(flag.ready.swap(false,Ordering::Relaxed),"no wake, benchmark deadlock"); polls+=1; match f.as_mut().poll(&mut cx) { Poll::Ready(v)=>return (v,polls,flag.wakes.load(Ordering::Relaxed)),Poll::Pending=>{} } } }
macro_rules! bench {
 ($func:ident,$c:literal,$o:literal,$p:ident,$req:ident,$reply:ident,$install:expr,$global:expr) => {
  fn $func(n: usize) {
   let setup=Instant::now(); let carrier=carrier::CarrierStorage::<1,16,64>::new(); let mut slab=[0;65536]; let mut storage=SessionKitStorage::uninit();let kit=storage.init();let sid=SessionId::new(2);let rv=kit.rendezvous(&mut slab,carrier.bind(sid).unwrap()).unwrap();
   let global=$global; let cp=project::<$c,_>(&global);let op=project::<$o,_>(&global); let mut c=rv.enter(sid,&cp).unwrap();let mut o=rv.enter(sid,&op).unwrap();let setup_us=setup.elapsed().as_micros();
   let run=Instant::now();let (result,polls,wakes)=drive(async {
    let client=async {let install=$install;c.send::<$p::Install>(&install).await?;assert_eq!(c.recv::<$p::Installed>().await?,install);for i in 0..n {let wire=(i as u128).to_be_bytes();c.send::<$p::$req>(&wire).await?;let b=c.offer().await?;assert_eq!(b.recv::<$p::$reply>().await?,wire);c.send::<$p::ResultTaken>(&wire).await?;}let wire=[0;16];c.send::<$p::RetireRequested>(&wire).await?;assert_eq!(c.recv::<$p::Retired>().await?,wire);c.send::<$p::RetirementAcknowledged>(&wire).await?;Ok::<(),hibana::EndpointError>(())};
    let owner=async {let install=o.recv::<$p::Install>().await?;o.send::<$p::Installed>(&install).await?;for _ in 0..n {let b=o.offer().await?;let wire=b.recv::<$p::$req>().await?;o.send::<$p::$reply>(&wire).await?;assert_eq!(o.recv::<$p::ResultTaken>().await?,wire);}let b=o.offer().await?;let wire=b.recv::<$p::RetireRequested>().await?;o.send::<$p::Retired>(&wire).await?;assert_eq!(o.recv::<$p::RetirementAcknowledged>().await?,wire);Ok::<(),hibana::EndpointError>(())};
    runtime::join2(client,owner).await
   }); result.unwrap(); let elapsed=run.elapsed();println!("{} n={} setup_us={} elapsed_us={} us_per_request={:.3} polls={} wakes={}",stringify!($func),n,setup_us,elapsed.as_micros(),elapsed.as_secs_f64()*1e6/n as f64,polls,wakes);
  }
 }
}
fn tiny_choreography()->g::Program<g::Seq<g::Send<24,25,tls::Install>,g::Seq<g::Send<25,24,tls::Installed>,g::Seq<g::Roll<g::Route<tls::HeaderMaskFlow<24,25>,g::Send<24,25,tls::RetireRequested>>>,g::Seq<g::Send<25,24,tls::Retired>,g::Send<24,25,tls::RetirementAcknowledged>>>>>> {g::seq(g::send::<24,25,tls::Install>(),g::seq(g::send::<25,24,tls::Installed>(),g::seq(g::route(g::seq(g::send::<24,25,tls::HeaderMask>(),g::seq(g::route(g::send::<25,24,tls::HeaderMaskReady>(),g::send::<25,24,tls::HeaderMaskRejected>()),g::send::<24,25,tls::ResultTaken>())),g::send::<24,25,tls::RetireRequested>()).roll(),g::seq(g::send::<25,24,tls::Retired>(),g::send::<24,25,tls::RetirementAcknowledged>()))))}
bench!(tiny_mask,24,25,tls,HeaderMask,HeaderMaskReady,[0u8;16],tiny_choreography());
bench!(key_mask,16,17,key,HeaderMask,HeaderMaskReady,0u64,key::key_choreography::<16,17>());
bench!(tls_mask,24,25,tls,HeaderMask,HeaderMaskReady,[0u8;16],tls::tls_choreography::<24,25>());
bench!(tls_crypto,24,25,tls,CryptoInput,CryptoAccepted,[0u8;16],tls::tls_choreography::<24,25>());
bench!(tls_early,24,25,tls,OpenEarly,EarlyOpened,[0u8;16],tls::tls_choreography::<24,25>());
fn main(){let args:Vec<_>=std::env::args().collect();let n=args.get(1).map(|s|s.parse().unwrap()).unwrap_or(100);let mode=args.get(2).map(String::as_str).unwrap_or("all"); if mode=="all"||mode=="tiny"{tiny_mask(n)}if mode=="all"||mode=="key"{key_mask(n)}if mode=="all"||mode=="tls"{tls_mask(n)}if mode=="all"||mode=="crypto"{tls_crypto(n)}if mode=="all"||mode=="early"{tls_early(n)}}
