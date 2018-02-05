// See https://tools.ietf.org/html/rfc1035#section-3.2.1.

#[derive(Copy, Clone, Debug)]
pub struct Zone {
    pub domain: &'str,
    pub records: Vec<Record>,
}

#[derive(Copy, Clone, Debug)]
pub struct Record {
    pub name: &'str,
    pub type: Type,
    pub class: Option<Class>,
    pub ttl: Option<i32>,
    pub rdata: &'str,
}

pub enum Type {
    A,
    NS,
    MD,
    MF,
    CNAME,
    SOA,
    MB,
    MG,
    MR,
    NULL,
    WKS,
    PTR,
    HINFO,
    MINFO,
    MX,
    TXT
}

pub enum Class {
    IN,
    CS,
    CH,
    HS
}